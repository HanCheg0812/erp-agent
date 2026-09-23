// Explicit, synthetic acceptance against the configured model and dedicated RustFS.
import assert from 'node:assert/strict';
import { randomUUID, randomBytes, createHash } from 'node:crypto';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { mkdir, writeFile, readFile, readdir } from 'node:fs/promises';
import pg from 'pg';

assert.equal(process.env.LIVE_ACCEPTANCE, '1', 'Set LIVE_ACCEPTANCE=1 to call the configured model');
const out = process.env.LIVE_ACCEPTANCE_OUTPUT ?? '/tmp/erp-rust-live'; await mkdir(out, { recursive: true });
const attempt = new Date().toISOString().replaceAll(':', '-');
const port = 3200;
const origin = `http://127.0.0.1:${port}`;
assert.equal(await fetch(`${origin}/health`).then(() => true, () => false), false, 'Port already occupied');
const binary = new URL('../services/api/target/debug/erp-agent-api', import.meta.url).pathname;
const db = new pg.Client({ connectionString: process.env.DATABASE_URL }); await db.connect();
const resume = process.argv.includes('--resume');
const automatic = process.env.LIVE_AUTO_COMPACTION === '1';
const report = resume ? JSON.parse(await readFile(`${out}/report.json`, 'utf8')) : { passed: false, runtimeFiles: [], objects: [] };
report.passed = false;
report.compactionMode = automatic ? 'automatic' : 'manual';
report.compactionSoftRatio = process.env.AGENT_COMPACTION_SOFT_RATIO ?? '0.80';
let child; let logs = ''; let token;
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
async function start() {
  child = spawn(binary, [], { env: { ...process.env, API_PORT: String(port) }, stdio: ['ignore', 'ignore', 'pipe'] });
  child.stderr.on('data', c => { logs += c; });
  for (let i = 0; i < 100; i++) { if (await fetch(`${origin}/ready`).then(r => r.ok, () => false)) return; await pause(100); }
  throw new Error('Rust API did not start');
}
async function stop() { if (child?.exitCode === null) { child.kill('SIGTERM'); await once(child, 'exit'); } }
async function api(path, body, options = {}) {
  const r = await fetch(`${origin}${path.startsWith('/internal/') ? '' : '/v1'}${path}`, { method: options.method ?? (body ? 'POST' : 'GET'), headers: { 'content-type': 'application/json', authorization: `Bearer ${options.token ?? token}` }, ...(body ? { body: JSON.stringify(body) } : {}) });
  assert.equal(r.status, options.status ?? (r.status === 201 ? 201 : 200), `${path}: ${await (!r.ok ? r.clone().text() : Promise.resolve(''))}`);
  return options.raw ? Buffer.from(await r.arrayBuffer()) : r.json();
}
async function turn(text, compact = false) {
  const result = await api(`/cases/${report.caseId}/${compact ? 'compactions' : 'conversation/messages'}`, compact ? { request_id: randomUUID() } : { client_message_id: randomUUID(), text });
  const start = performance.now(); let firstDelta;
  for (let i = 0; i < 360; i++) {
    const history = await api(`/cases/${report.caseId}/conversation`);
    const message = history.items.find(m => m.id === result.message.id);
    const response = message?.response;
    if (response?.text && firstDelta === undefined) firstDelta = Math.round(performance.now() - start);
    if (['completed', 'failed'].includes(response?.state)) {
      assert.equal(response.state, 'completed', `Agent failed: ${response.error}`);
      report.sessionId = history.session_id;
      return { ...response, steps: undefined, firstTextMs: firstDelta, elapsedMs: Math.round(performance.now() - start) };
    }
    for (const dir of (await readdir('/tmp')).filter(n => n.startsWith('erp-agent-turn-'))) {
      for (const file of await readdir(`/tmp/${dir}`, { recursive: true }).catch(() => [])) if (!report.runtimeFiles.includes(file)) report.runtimeFiles.push(file);
    }
    await pause(500);
  }
  throw new Error('Agent timed out');
}
try {
  await start();
  token = (await api('/auth/login', { email: process.env.DEMO_EMAIL, password: process.env.DEMO_PASSWORD })).access_token;
  if (!resume) {
  const record = await api('/cases', { title: 'Context compression acceptance', customer_code: 'SYNTHETIC', part_number: 'TEST-42', revision: 'C', demand_type: 'order', quantity_basis: 'replacement' });
  report.caseId = record.id;
  const memoryText = 'Synthetic acceptance preference: display quantities in units. Reference preference code SILVER-638.';
  const memory = (await api('/memory')).items.find(item => item.text === memoryText)
    ?? await api('/memory', { request_id: randomUUID(), text: memoryText });
  report.memoryId = memory.id;
  const other = (await api('/auth/register', { email: `isolation-${randomBytes(6).toString('hex')}@example.test`, password: randomBytes(24).toString('base64url') })).access_token;
  for (const [filename, content_type, bytes] of [
    ['synthetic-order.txt', 'text/plain', Buffer.from('SYNTHETIC ORDER: 1280 units, revision C, due 2026-10-22.')],
    ['synthetic-pixel.png', 'image/png', Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aK1kAAAAASUVORK5CYII=', 'base64')],
  ]) {
    const input = { request_id: randomUUID(), filename, content_type, data_base64: bytes.toString('base64') };
    const file = await api(`/cases/${record.id}/documents`, input);
    assert.equal((await api(`/cases/${record.id}/documents`, input)).id, file.id);
    assert.deepEqual(await api(`/documents/${file.id}`, undefined, { raw: true }), bytes);
    await api(`/documents/${file.id}`, undefined, { token: other, status: 404 });
    report.objects.push({ id: file.id, filename, sha256: createHash('sha256').update(bytes).digest('hex') });
  }
  assert.equal((await api('/memory', undefined, { token: other })).items.length, 0);
  console.log('PASS RustFS private upload/download, idempotency, content hashes, cross-user denial');
  report.first = await turn('请读取当前案件，简洁回答它的版本与数量口径，以及我保存的个人数量显示偏好及其代号。不要修改订单。');
  assert.match(report.first.text, /SILVER-638/);
  assert(report.first.tools.some(t => t.name === 'case_read' && t.state === 'completed'));
  console.log('PASS real Rust worker -> JCode -> Responses, case tool and personal memory');
  report.noop = await turn('', true);
  assert.equal(report.noop.compactions.length, 0); assert.match(report.noop.text, /Context unchanged/);
  const claim = await api(`/agent-sessions/${report.sessionId}/claim`, {});
  const current = await api(`/internal/agent-sessions/${report.sessionId}`, undefined, { token: claim.storage_token });
  const { messages, env_snapshots, memory_injections, replay_events, ...metadata } = current.state;
  const stored = (role, content) => ({ id: randomUUID(), role, content, timestamp: new Date().toISOString() });
  const fact = 'The synthetic case checkpoint is QUARTZ-914. Replace the TOTAL with 1280 units, due 2026-10-22, revision C. This is NOT an increment. Evidence is delivery-note-R3 page 2 table 1. No ERP write or approval has occurred.';
  const seed = [stored('user', [{ type: 'text', text: fact }]), stored('assistant', [{ type: 'text', text: 'Recorded for this synthetic review; approval remains pending.' }])];
  for (let i = 0; i < 18; i++) {
    const call = `fixture-${i}`;
    seed.push(stored('assistant', [{ type: 'tool_use', id: call, name: 'case_read', input: {} }]));
    seed.push(stored('user', [{ type: 'tool_result', tool_use_id: call, content: `Synthetic inspection ${i}: ` + ('Unchanged intermediate worksheet: production planning remains under review; no committed stock allocation, no purchase, no delivery confirmation. '.repeat(24)), is_error: false }]));
    seed.push(stored('assistant', [{ type: 'text', text: `Inspection ${i} is informational only. Continue validating before approval.` }]));
  }
  await api(`/internal/agent-sessions/${report.sessionId}/deltas`, { request_id: randomUUID(), expected_revision: current.revision, metadata, vectors: Object.fromEntries(['messages', 'env_snapshots', 'memory_injections', 'replay_events'].map(k => [k, { mode: 'append', values: k === 'messages' ? seed : [] }])) }, { token: claim.storage_token });
  report.seededMessages = messages.length + seed.length;
  }
  report.compaction = automatic
    ? await turn('这是自动上下文压缩验收。不要调用工具，只回复：AUTO-COMPACTION-CONTINUE。')
    : await turn('', true);
  assert.equal(report.compaction.compactions.length, 1);
  const metric = report.compaction.compactions[0]; assert(metric.tokens_saved > 0); assert(metric.messages_compacted > 0);
  if (automatic) assert.equal(metric.trigger, 'reactive');
  const persisted = (await db.query('SELECT metadata FROM agent_sessions WHERE id=$1', [report.sessionId])).rows[0].metadata;
  const summary = persisted.compaction.summary_text;
  for (const fact of ['QUARTZ-914', '1280', '2026-10-22', 'delivery-note-R3']) assert(summary.includes(fact), `Lost fact ${fact}`);
  const all = (await db.query("SELECT e.payload FROM agent_session_entries e JOIN agent_session_heads h ON h.session_id=e.session_id AND h.kind=e.kind AND h.generation=e.generation WHERE e.session_id=$1 AND e.kind='messages' AND e.position<h.length ORDER BY e.position", [report.sessionId])).rows.map(r => r.payload);
  assert(all.length >= report.seededMessages, 'Original transcript was truncated');
  assert(all.some(message => JSON.stringify(message).includes('QUARTZ-914')), 'Original checkpoint message is missing');
  const tail = all.slice(persisted.compaction.compacted_count); const callIds = new Set();
  for (const message of tail) for (const block of message.content) { if (block.type === 'tool_use') callIds.add(block.id); if (block.type === 'tool_result') assert(callIds.has(block.tool_use_id), 'Orphan tool result in active tail'); }
  report.summary = summary; report.compactedCount = persisted.compaction.compacted_count; report.persistedMessages = all.length;
  console.log(`PASS real compaction: ${metric.pre_tokens} -> ${metric.post_tokens} estimated tokens; ${all.length} original messages preserved`);
  await stop(); await start();
  for (const file of report.objects) assert.equal(createHash('sha256').update(await api(`/documents/${file.id}`, undefined, { raw: true })).digest('hex'), file.sha256);
  report.recovered = await turn('请回忆历史记录中的 checkpoint、最终总量、交期、修订版和证据位置。它是增量吗？ERP 是否已执行？简洁回答。');
  for (const fact of ['QUARTZ-914', '1280', '2026-10-22']) assert(report.recovered.text.includes(fact), `Recovery lost ${fact}`);
  assert(!report.runtimeFiles.some(f => /(?:sessions|todos)\/.+\.jsonl?$/.test(f)));
  console.log('PASS API restart: summary, original transcript, document hashes and factual continuation');
  report.passed = true;
} finally {
  await stop(); await db.end();
  report.checked_at = new Date().toISOString();
  await writeFile(`${out}/report.json`, JSON.stringify(report, null, 2));
  await writeFile(`${out}/report-${attempt}.json`, JSON.stringify(report, null, 2));
  await writeFile(`${out}/api-${attempt}.log`, logs.replaceAll(process.env.DEEPSEEK_API_KEY ?? 'NO_SECRET', '[redacted]'), { mode: 0o600 });
  await writeFile(`${out}/api.log`, logs.replaceAll(process.env.DEEPSEEK_API_KEY ?? 'NO_SECRET', '[redacted]'), { mode: 0o600 });
}
