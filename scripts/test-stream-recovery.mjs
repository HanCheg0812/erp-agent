import assert from 'node:assert/strict';
import { randomBytes, randomUUID } from 'node:crypto';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { createServer } from 'node:http';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import pg from 'pg';

const apiBinary = new URL('../services/api/target/debug/erp-agent-api', import.meta.url).pathname;
const probeBinary = new URL('../services/runtime-bridge/target/debug/examples/stream-recovery-probe', import.meta.url).pathname;
const directory = await mkdtemp(join(tmpdir(), 'erp-stream-recovery-'));
const admin = new pg.Client({ connectionString: process.env.DATABASE_URL });
await admin.connect();
const name = `erp_stream_test_${randomBytes(8).toString('hex')}`;
await admin.query(`CREATE DATABASE ${name}`);
const url = new URL(process.env.DATABASE_URL); url.pathname = name;
const listener = createServer(); listener.listen(0, '127.0.0.1'); await once(listener, 'listening');
const port = listener.address().port; await new Promise(resolve => listener.close(resolve));
const origin = `http://127.0.0.1:${port}`;
const env = { ...process.env, DATABASE_URL: url.href, API_PORT: String(port), DEEPSEEK_API_KEY: '', CUBE_API_URL: '' };
let apiProcess; let token; let logs = '';
async function stop() {
  if (apiProcess?.exitCode === null) { apiProcess.kill('SIGTERM'); await once(apiProcess, 'exit'); }
}
async function start() {
  apiProcess = spawn(apiBinary, [], { env, stdio: ['ignore', 'ignore', 'pipe'] });
  apiProcess.stderr.on('data', c => { logs += c; });
  for (let i = 0; i < 100; i++) {
    if (await fetch(`${origin}/ready`).then(r => r.ok).catch(() => false)) return;
    await new Promise(resolve => setTimeout(resolve, 50));
  }
  throw Error(`API did not start: ${logs}`);
}
async function api(path, body, credential = token) {
  const response = await fetch(`${origin}${path}`, { method: body ? 'POST' : 'GET', headers: { 'content-type': 'application/json', ...(credential ? { authorization: `Bearer ${credential}` } : {}) }, ...(body ? { body: JSON.stringify(body) } : {}) });
  const value = await response.json(); assert(response.ok, `${path}: ${response.status}`); return value;
}
async function probe(scenario, session, credential) {
  const child = spawn(probeBinary, [scenario], { cwd: directory, env: { PATH: '/usr/bin:/bin', LANG: 'C.UTF-8', HOME: directory, JCODE_HOME: join(directory, 'jcode'), JCODE_NO_TELEMETRY: '1', DO_NOT_TRACK: '1', JCODE_MEMORY_ENABLED: 'false', JCODE_MEMORY_SIDECAR_ENABLED: 'false', JCODE_SESSION_STORE_URL: origin, JCODE_SESSION_STORE_ID: session, JCODE_SESSION_STORE_TOKEN: credential }, stdio: ['ignore', 'pipe', 'pipe'] });
  let output = ''; let errors = ''; let killed = false;
  child.stdout.on('data', data => {
    output += data;
    if (scenario === 'crash' && output.includes('visible') && !killed) { killed = true; child.kill('SIGKILL'); }
  });
  child.stderr.on('data', data => { errors += data; });
  const watchdog = setTimeout(() => child.kill('SIGKILL'), 25000);
  try {
    const [code, signal] = await once(child, 'exit');
    if (scenario === 'crash') { assert(killed); assert.equal(signal, 'SIGKILL'); }
    else { assert.equal(code, 0, `${scenario}: ${errors}`); assert(output.includes(`PASS ${scenario}`)); }
  } finally { clearTimeout(watchdog); }
}
try {
  const migration = spawn(apiBinary, ['migrate'], { env, stdio: 'pipe' });
  assert.equal((await once(migration, 'exit'))[0], 0);
  await start();
  token = (await api('/v1/auth/register', { email: 'stream@example.test', password: randomBytes(24).toString('hex') })).access_token;
  for (const scenario of ['normal', 'stream_error', 'event_error', 'eof', 'stop', 'timeout', 'crash']) {
    const record = await api('/v1/conversations', { request_id: randomUUID() });
    const session = (await api(`/v1/cases/${record.id}/agent-sessions`, {})).id;
    let lease = await api(`/v1/agent-sessions/${session}/claim`, {});
    await probe(scenario, session, lease.storage_token);
    await stop(); await start();
    lease = await api(`/v1/agent-sessions/${session}/claim`, {});
    await probe('recall', session, lease.storage_token);
    const saved = await api(`/internal/agent-sessions/${session}`, undefined, lease.storage_token);
    assert.equal(saved.state.messages.filter(m => m.role === 'assistant' && m.content.some(b => b.text === 'Durable partial 7391')).length, 1);
    assert.equal((await api(`/internal/agent-sessions/${session}/documents/stream_checkpoint`, undefined, lease.storage_token)).payload, null);
    console.log(`PASS ${scenario}: visible text survives API/process restart, reaches next provider call exactly once`);
  }
} finally {
  await stop();
  await admin.query(`DROP DATABASE ${name} WITH (FORCE)`); await admin.end();
  await rm(directory, { recursive: true, force: true });
}
