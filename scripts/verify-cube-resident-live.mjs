import assert from 'node:assert/strict';
import { createHmac, randomUUID } from 'node:crypto';
import { mkdir, writeFile } from 'node:fs/promises';
import pg from 'pg';

const origin = process.env.VERIFY_API_ORIGIN ?? 'http://127.0.0.1:3100';
const count = Number(process.env.VERIFY_CUBE_COUNT ?? 8);
const timeoutMs = Number(process.env.VERIFY_CUBE_TIMEOUT_MS ?? 300000);
const output = process.env.VERIFY_CUBE_OUTPUT ?? `/tmp/erp-cube-acceptance/${Date.now()}`;
assert(Number.isInteger(count) && count > 0 && count <= 50);
const db = new pg.Client({ connectionString: process.env.DATABASE_URL });
let token;
const cases = [];
const samples = [];

async function api(path, method = 'GET', body) {
  const response = await fetch(`${origin}/v1${path}`, {
    method,
    headers: { 'content-type': 'application/json', ...(token ? { authorization: `Bearer ${token}` } : {}) },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });
  const data = await response.json();
  if (!response.ok) throw new Error(`${method} ${path}: ${response.status} ${JSON.stringify(data)}`);
  return data;
}

const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const percentile = (values, p) => {
  if (!values.length) return null;
  const ordered = [...values].sort((a, b) => a - b);
  return Math.round(ordered[Math.ceil(p * ordered.length) - 1]);
};

async function verifyCaseTokenIsolation(first, second) {
  const secret = process.env.CUBE_RUNTIME_SHARED_SECRET;
  assert(secret && secret.length >= 32, 'Cube runtime secret is required for the authorization probe');
  const proxy = process.env.CUBE_PROXY_URL?.replace(/\/$/, '');
  assert(proxy, 'Cube proxy is required for the authorization probe');
  const port = process.env.CUBE_RUNTIME_PORT ?? '49999';
  const base = `${proxy}/sandbox/${second.sandbox_id}/${port}`;
  const ready = await fetch(`${base}/health/ready`);
  assert.equal(ready.status, 200, 'Target Bridge must be reachable');
  const wrongCaseToken = createHmac('sha256', secret)
    .update('erp-agent/cube-runtime/v1/')
    .update(Buffer.from(first.operation.replaceAll('-', ''), 'hex'))
    .digest('hex');
  const rejected = await fetch(`${base}/v1/reconnect`, {
    method: 'POST', headers: { 'x-agent-runtime-token': wrongCaseToken },
  });
  assert.equal(rejected.status, 401, 'A different case token must be rejected');
  return { bridge_ready_status: ready.status, wrong_case_token_status: rejected.status };
}

async function runRound(name, prompt) {
  const submittedAt = Date.now();
  const sent = await Promise.allSettled(cases.map(async entry => {
    const result = await api(`/cases/${entry.id}/conversation/messages`, 'POST', {
      client_message_id: randomUUID(),
      settings: { approval: 'ask', model: 'deepseek-flash', effort: 'low' },
      text: prompt,
    });
    return { case_id: entry.id, message_id: result.message.id, sent_at: Date.now() };
  }));
  const messages = sent.map((result, index) => result.status === 'fulfilled'
    ? result.value
    : { case_id: cases[index].id, error: String(result.reason) });
  const pending = new Set(messages.filter(item => item.message_id).map(item => item.case_id));
  const deadline = Date.now() + timeoutMs;
  while (pending.size && Date.now() < deadline) {
    const states = await db.query(
      "SELECT state,count(*)::int AS count FROM agent_runs WHERE case_id=ANY($1::uuid[]) AND state IN ('queued','running','waiting') GROUP BY state",
      [cases.map(entry => entry.id)],
    );
    samples.push({ round: name, elapsed_ms: Date.now() - submittedAt, ...Object.fromEntries(states.rows.map(row => [row.state, row.count])) });
    await Promise.all([...pending].map(async id => {
      const history = await api(`/cases/${id}/conversation`);
      const message = messages.find(item => item.case_id === id);
      const response = history.items.find(item => item.id === message.message_id)?.response;
      if (response && ['completed', 'failed', 'cancelled'].includes(response.state)) {
        message.state = response.state;
        message.error = response.error;
        message.tool_names = response.tools?.map(tool => `${tool.name}:${tool.state}`);
        message.text_excerpt = response.text?.slice(0, 220);
        message.finished_at = Date.now();
        pending.delete(id);
      }
    }));
    if (pending.size) await sleep(1000);
  }
  for (const message of messages) if (pending.has(message.case_id)) message.state = 'timeout';
  const ids = messages.filter(message => message.message_id).map(message => message.message_id);
  if (ids.length) {
    const timings = await db.query(`
      SELECT message_id::text,
        round(extract(epoch from started_at - m.created_at)*1000)::int AS queue_ms,
        round(extract(epoch from first_token_at - started_at)*1000)::int AS first_token_after_start_ms,
        round(extract(epoch from first_token_at - m.created_at)*1000)::int AS first_token_after_submit_ms,
        round(extract(epoch from finished_at - m.created_at)*1000)::int AS total_ms,
        r.state, r.error
      FROM agent_runs r JOIN case_conversation_messages m ON m.id=r.message_id
      WHERE r.message_id::text=ANY($1::text[])`, [ids]);
    for (const timing of timings.rows) Object.assign(messages.find(item => item.message_id === timing.message_id), timing);
  }
  return {
    name, submitted_at: new Date(submittedAt).toISOString(), messages,
    summary: {
      completed: messages.filter(item => item.state === 'completed').length,
      failed: messages.filter(item => item.state !== 'completed').length,
      peak_running: Math.max(0, ...samples.filter(item => item.round === name).map(item => item.running ?? 0)),
      queue_p50_ms: percentile(messages.map(item => item.queue_ms).filter(Number.isFinite), 0.5),
      queue_p95_ms: percentile(messages.map(item => item.queue_ms).filter(Number.isFinite), 0.95),
      first_token_p50_ms: percentile(messages.map(item => item.first_token_after_submit_ms).filter(Number.isFinite), 0.5),
      first_token_p95_ms: percentile(messages.map(item => item.first_token_after_submit_ms).filter(Number.isFinite), 0.95),
      total_p50_ms: percentile(messages.map(item => item.total_ms).filter(Number.isFinite), 0.5),
      total_p95_ms: percentile(messages.map(item => item.total_ms).filter(Number.isFinite), 0.95),
    },
  };
}

await db.connect();
const report = { count, origin, rounds: [], cases, samples, cleanup: [] };
try {
  token = (await api('/auth/login', 'POST', {
    email: process.env.DEMO_EMAIL, password: process.env.DEMO_PASSWORD,
  })).access_token;
  const created = await Promise.all(Array.from({ length: count }, () =>
    api('/conversations', 'POST', { request_id: randomUUID() })));
  for (const item of created) cases.push({ id: item.id });
  report.rounds.push(await runRound('cold', '请使用 ERP 只读工具列出我有权限的订单，并读取列表中的第一张订单。只需回答真实订单号和数量，不修改业务数据。'));
  if (report.rounds[0].summary.completed === count) {
    for (const entry of cases) {
      const state = await api(`/cases/${entry.id}/sandbox`);
      entry.operation = state.operation?.id;
      entry.sandbox_id = state.operation?.sandbox_id;
      entry.session_id = (await api(`/cases/${entry.id}/conversation`)).session_id;
    }
    if (cases.length > 1) report.authorization = await verifyCaseTokenIsolation(cases[0], cases[1]);
    report.rounds.push(await runRound('warm', '请复述刚才查询到的 ERP 订单号和数量。不要再次调用工具。'));
  }
  const bindings = await db.query(
    "SELECT case_id::text,session_id,sandbox_id,state FROM case_sandboxes WHERE case_id=ANY($1::uuid[]) AND state<>'released'",
    [cases.map(entry => entry.id)],
  );
  report.bindings = bindings.rows;
  report.isolated = bindings.rows.length === count &&
    new Set(bindings.rows.map(row => row.sandbox_id)).size === count &&
    new Set(bindings.rows.map(row => row.session_id)).size === count;
} catch (error) {
  report.error = String(error);
} finally {
  await mkdir(output, { recursive: true });
  await writeFile(`${output}/report.json`, JSON.stringify(report, null, 2));
  console.log(JSON.stringify({ report: `${output}/report.json`, rounds: report.rounds.map(round => round.summary), isolated: report.isolated, error: report.error }));
  for (const entry of cases) {
    try {
      const state = await api(`/cases/${entry.id}/sandbox`);
      if (state.operation && state.operation.state !== 'released') {
        const released = await api(`/cases/${entry.id}/sandbox/${state.operation.id}`, 'DELETE');
        report.cleanup.push({ case_id: entry.id, state: released.operation?.state });
      }
    } catch (error) { report.cleanup.push({ case_id: entry.id, error: String(error) }); }
  }
  await writeFile(`${output}/report.json`, JSON.stringify(report, null, 2));
  await db.end();
}
assert(!report.error, report.error);
assert(report.rounds.every(round => round.summary.completed === count), 'Some Cube turns did not complete');
assert(report.isolated, 'Case sandboxes or sessions were not isolated');
