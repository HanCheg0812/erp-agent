import assert from 'node:assert/strict';
import { randomBytes, randomUUID } from 'node:crypto';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { createServer } from 'node:net';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import pg from 'pg';

const users = Number(process.argv.find(value => value.startsWith('--users='))?.slice(8) ?? 50);
const workers = Number(process.argv.find(value => value.startsWith('--workers='))?.slice(10) ?? 1);
const concurrency = Number(process.argv.find(value => value.startsWith('--concurrency='))?.slice(14) ?? 4);
const rounds = Number(process.argv.find(value => value.startsWith('--rounds='))?.slice(9) ?? 1);
const live = process.argv.includes('--live');
const maxWaitMs = Number(process.argv.find(value => value.startsWith('--max-wait-ms='))?.slice(14) ?? (live ? 300000 : 60000));
assert(Number.isInteger(users) && users > 0 && users <= 200);
assert(Number.isInteger(workers) && workers > 0 && workers <= 8);
assert(Number.isInteger(concurrency) && concurrency > 0 && concurrency <= 8);
assert(Number.isInteger(rounds) && rounds > 0 && rounds <= 3);
assert(Number.isInteger(maxWaitMs) && maxWaitMs >= 1000 && maxWaitMs <= 600000);

const root = new URL('..', import.meta.url).pathname;
const binary = join(root, 'services/api/target/debug/erp-agent-api');
const stub = join(root, 'scripts/fixtures/runtime-stub.mjs');
const runtime = join(root, 'services/runtime-bridge/target/debug/erp-agent-runtime');
const fixtureDir = live ? null : await mkdtemp(join(tmpdir(), 'erp-worker-bench-'));
const wrapper = fixtureDir && join(fixtureDir, 'runtime');
const quote = value => `'${value.replaceAll("'", "'\\''")}'`;
if (wrapper) await writeFile(wrapper, `#!/bin/sh\nexec ${quote(process.execPath)} ${quote(stub)}\n`, { mode: 0o700 });

const admin = new pg.Client({ connectionString: process.env.DATABASE_URL });
await admin.connect();
const database = `erp_worker_bench_${randomBytes(8).toString('hex')}`;
const url = new URL(process.env.DATABASE_URL);
url.pathname = database;
const processes = [];
let db;
let notificationClient;
let notifications = 0;

async function freePort() {
  const server = createServer();
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  const port = server.address().port;
  await new Promise(resolve => server.close(resolve));
  return port;
}

function percentile(values, ratio) {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.ceil(sorted.length * ratio) - 1];
}

async function ready(origin, process) {
  for (let attempt = 0; attempt < 100; attempt++) {
    if (process.exitCode !== null) throw Error('API exited before readiness');
    if (await fetch(`${origin}/ready`).then(response => response.ok).catch(() => false)) return;
    await new Promise(resolve => setTimeout(resolve, 50));
  }
  throw Error('API readiness timed out');
}

try {
  await admin.query(`CREATE DATABASE ${database}`);
  db = new pg.Client({ connectionString: url.href });
  await db.connect();
  notificationClient = new pg.Client({ connectionString: url.href });
  await notificationClient.connect();
  notificationClient.on('notification', () => { notifications++; });
  await notificationClient.query('LISTEN erp_agent_runs');
  const port = await freePort();
  const origin = `http://127.0.0.1:${port}`;
  const baseEnv = {
    ...process.env,
    DATABASE_URL: url.href,
    ...(!live ? { DEEPSEEK_API_KEY: 'synthetic-test-only' } : {}),
    AGENT_RUNTIME_BINARY: live ? runtime : wrapper,
    AGENT_STORAGE_ORIGIN: origin,
    AGENT_MAX_CONCURRENT_RUNS: String(concurrency),
    API_HOST: '127.0.0.1',
  };
  const migration = spawn(binary, ['migrate'], { env: { ...baseEnv, API_PORT: String(port) }, stdio: ['ignore', 'ignore', 'pipe'] });
  let errors = '';
  migration.stderr.on('data', chunk => { errors += chunk; });
  assert.equal((await once(migration, 'exit'))[0], 0, errors);

  const apiProcess = spawn(binary, [], {
    env: { ...baseEnv, API_PORT: String(port), AGENT_WORKER_ENABLED: '0' },
    stdio: ['ignore', 'ignore', 'pipe'],
  });
  apiProcess.stderr.on('data', chunk => { errors += chunk; });
  processes.push(apiProcess);
  await ready(origin, apiProcess);

  for (let index = 0; index < workers; index++) {
    const process = spawn(binary, ['worker'], {
      env: { ...baseEnv, API_PORT: String(port) },
      stdio: ['ignore', 'ignore', 'pipe'],
    });
    process.stderr.on('data', chunk => { errors += chunk; });
    processes.push(process);
  }

  for (let attempt = 0; attempt < 100; attempt++) {
    if (processes.some(process => process.exitCode !== null)) throw Error(`Worker exited before readiness: ${errors}`);
    const listeners = Number((await db.query("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND query='LISTEN erp_agent_runs' AND state='idle'")).rows[0].count);
    if (listeners >= workers) break;
    if (attempt === 99) throw Error(`Timed out waiting for ${workers} worker listeners: ${errors}`);
    await new Promise(resolve => setTimeout(resolve, 50));
  }

  async function api(path, body, token) {
    const response = await fetch(`${origin}/v1${path}`, {
      method: 'POST',
      headers: { 'content-type': 'application/json', ...(token ? { authorization: `Bearer ${token}` } : {}) },
      body: JSON.stringify(body),
    });
    const value = await response.json();
    assert(response.ok, `${path}: ${response.status} ${JSON.stringify(value)} ${errors.slice(-2000)}`);
    return value;
  }

  const actors = [];
  for (let index = 0; index < users; index += 3) {
    actors.push(...await Promise.all(Array.from({ length: Math.min(3, users - index) }, async () => {
      const token = (await api('/auth/register', {
        email: `worker-bench-${randomUUID()}@example.test`,
        password: randomBytes(24).toString('hex'),
      })).access_token;
      const caseId = (await api('/conversations', { request_id: randomUUID() }, token)).id;
      return { token, caseId };
    })));
  }

  async function runRound(round) {
    const notificationsBefore = notifications;
    const submittedAt = Date.now();
    const sent = await Promise.all(actors.map(async ({ token, caseId }, index) => {
    const began = Date.now();
    const value = await api(`/cases/${caseId}/conversation/messages`, {
      client_message_id: randomUUID(),
      text: live
        ? `Answer this arithmetic question with one integer. Calculate (17 * 19) + ${index} - ${index}. Do not use tools.`
        : `parallel:${users}:${round}:${index}`,
      settings: { approval: 'ask', model: 'deepseek-flash', effort: live ? 'low' : 'none' },
    }, token);
    return { id: value.message.id, enqueueMs: Date.now() - began };
    }));
    const ids = sent.map(item => item.id);
    let rows = [];
    const progress = [];
    const firstReasoningAt = new Map();
    let peakDbConnections = 0;
    let lastSample = -1;
    for (let attempt = 0; attempt < Math.ceil(maxWaitMs / 100); attempt++) {
    rows = (await db.query('SELECT r.message_id,r.state,r.text,r.steps,r.error,m.created_at,r.started_at,r.first_token_at,r.finished_at FROM agent_runs r JOIN case_conversation_messages m ON m.id=r.message_id WHERE r.message_id=ANY($1::bigint[])', [ids])).rows;
    const observedAt = Date.now();
    peakDbConnections = Math.max(peakDbConnections, Number((await db.query('SELECT count(*) FROM pg_stat_activity WHERE datname=current_database()')).rows[0].count));
    if (live) for (const row of rows) {
      if (!firstReasoningAt.has(row.message_id) && row.steps.some(step => step.kind === 'reasoning' && step.text?.trim())) {
        firstReasoningAt.set(row.message_id, observedAt);
      }
    }
    const started = rows.filter(row => row.started_at).length;
    if (started !== lastSample) {
      progress.push([Date.now() - submittedAt, started]);
      lastSample = started;
    }
    if (rows.length === users && rows.every(row => ['completed', 'failed', 'cancelled'].includes(row.state))) break;
    await new Promise(resolve => setTimeout(resolve, 100));
    }
    assert.equal(rows.length, users);
    const terminal = rows.filter(row => ['completed', 'failed', 'cancelled'].includes(row.state));
    const completed = rows.filter(row => row.state === 'completed');
    const ms = value => new Date(value).getTime();
    const queue = completed.map(row => ms(row.started_at) - ms(row.created_at));
    const first = completed.map(row => ms(row.first_token_at) - ms(row.created_at));
    const total = completed.map(row => ms(row.finished_at) - ms(row.created_at));
    const service = completed.map(row => ms(row.finished_at) - ms(row.started_at));
    const reasoning = completed.filter(row => firstReasoningAt.has(row.message_id)).map(row => firstReasoningAt.get(row.message_id) - ms(row.created_at));
    const events = completed.flatMap(row => [[ms(row.started_at), 1], [ms(row.finished_at), -1]])
    .sort((a, b) => a[0] - b[0] || a[1] - b[1]);
    let active = 0;
    let peak = 0;
    for (const [, change] of events) { active += change; peak = Math.max(peak, active); }
    const result = {
      users, workers, concurrency_per_worker: concurrency, round, completed: completed.length,
      failed: users - completed.length, terminal: terminal.length, peak,
      enqueue_p50_ms: percentile(sent.map(item => item.enqueueMs), .5),
      enqueue_p95_ms: percentile(sent.map(item => item.enqueueMs), .95),
      queue_p50_ms: percentile(queue, .5), queue_p95_ms: percentile(queue, .95),
      queue_under_500_ms: queue.filter(value => value < 500).length,
      queue_under_1500_ms: queue.filter(value => value < 1500).length,
      queue_under_3000_ms: queue.filter(value => value < 3000).length,
      service_p50_ms: percentile(service, .5), service_p95_ms: percentile(service, .95),
      first_token_p50_ms: percentile(first, .5), first_token_p95_ms: percentile(first, .95),
      ...(live ? {
        mode: 'real DeepSeek Flash, low reasoning', reasoning_visible_count: reasoning.length,
        correct_answer_count: completed.filter(row => /\b323\b/.test(row.text)).length,
        answer_mismatches: completed.filter(row => !/\b323\b/.test(row.text)).map(row => row.text.slice(0, 120)),
        reasoning_visible_p50_ms: reasoning.length ? percentile(reasoning, .5) : null,
        reasoning_visible_p95_ms: reasoning.length ? percentile(reasoning, .95) : null,
        reasoning_sample_interval_ms: 100,
        failure_reasons: [...new Set(rows.filter(row => row.state === 'failed').map(row => String(row.error ?? 'unknown').slice(0, 200)))],
      } : {}),
      total_p50_ms: percentile(total, .5), total_p95_ms: percentile(total, .95),
      batch_wall_ms: Date.now() - submittedAt,
      pg_notifications: notifications - notificationsBefore,
      peak_db_connections: peakDbConnections,
      worker_stderr: live ? undefined : errors.slice(-500),
      progress,
    };
    console.log(JSON.stringify(result));
    if (!live) assert.equal(completed.length, users, errors.slice(-3000));
  }
  for (let round = 1; round <= rounds; round++) await runRound(round);
} finally {
  for (const process of processes) process.kill('SIGTERM');
  await Promise.all(processes.map(process => process.exitCode === null ? once(process, 'exit') : null));
  if (db) await db.end();
  if (notificationClient) await notificationClient.end();
  await admin.query(`DROP DATABASE IF EXISTS ${database} WITH (FORCE)`);
  await admin.end();
  if (fixtureDir) await rm(fixtureDir, { recursive: true, force: true });
}
