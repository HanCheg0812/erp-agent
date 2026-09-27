import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import { randomUUID } from 'node:crypto';
import pg from 'pg';

const origin = process.env.VERIFY_API_ORIGIN ?? 'http://127.0.0.1:3100';
const cubeOrigin = (process.env.CUBE_API_URL ?? 'http://127.0.0.1:3001').replace(/\/$/, '');
const output = process.env.VERIFY_BROWSER_OUTPUT ?? `/tmp/erp-cube-browser/${Date.now()}`;
const browserTemplate = process.env.CUBE_BROWSER_TEMPLATE_ID;
assert(browserTemplate, 'CUBE_BROWSER_TEMPLATE_ID is required');
const attempt = randomUUID();
const db = new pg.Client({ connectionString: process.env.DATABASE_URL });
const report = {
  started_at: new Date().toISOString(),
  browser_template: browserTemplate,
  attempt,
};
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
let token;
let caseId;
let passed = false;

async function api(path, method = 'GET', body) {
  const response = await fetch(`${origin}/v1${path}`, {
    method,
    headers: {
      'content-type': 'application/json',
      ...(token ? { authorization: `Bearer ${token}` } : {}),
    },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });
  const data = await response.json();
  assert(response.ok, `${method} ${path}: ${response.status} ${JSON.stringify(data)}`);
  return data;
}

async function cube(path) {
  const response = await fetch(`${cubeOrigin}${path}`, {
    headers: process.env.CUBE_API_KEY ? { 'X-API-Key': process.env.CUBE_API_KEY } : {},
  });
  assert.equal(response.status, 200, `Cube ${path}: ${response.status}`);
  return response.json();
}

await mkdir(output, { recursive: true });
await db.connect();
try {
  token = (await api('/auth/login', 'POST', {
    email: process.env.DEMO_EMAIL,
    password: process.env.DEMO_PASSWORD,
  })).access_token;
  caseId = (await api('/conversations', 'POST', { request_id: randomUUID() })).id;
  report.case_id = caseId;
  const before = (await db.query(
    'SELECT count(*)::int AS count FROM cube_browser_tool_calls WHERE case_id=$1',
    [caseId],
  )).rows[0].count;

  const unauthorized = await fetch(`${origin}/internal/agent-sessions/not-a-real-session/cube/browser-browse`, {
    method: 'POST',
    headers: { 'content-type': 'application/json', authorization: 'Bearer invalid' },
    body: JSON.stringify({ tool_call_id: 'unauthorized_probe', url: 'https://example.com' }),
  });
  assert.equal(unauthorized.status, 401, 'A forged session must not run the browser tool');
  report.unauthorized_status = unauthorized.status;

  const submitted = await api(`/cases/${caseId}/conversation/messages`, 'POST', {
    client_message_id: randomUUID(),
    settings: { approval: 'ask', model: 'deepseek-flash', effort: 'low' },
    text: `Synthetic browser acceptance ${attempt}. Call cube_browser_browse exactly once with URL https://example.com. Do not call any other tools. Reply with TITLE=<title> and HEAD=<heading> from the tool result.`,
  });
  const messageId = submitted.message.id;
  report.message_id = messageId;
  const deadline = Date.now() + 240_000;
  let response;
  while (Date.now() < deadline) {
    const history = await api(`/cases/${caseId}/conversation`);
    response = history.items.find(item => item.id === messageId)?.response;
    if (response && ['completed', 'failed', 'cancelled'].includes(response.state)) break;
    await sleep(1000);
  }
  assert(response, 'Agent browser turn timed out');
  report.turn = {
    state: response.state,
    text: response.text,
    error: response.error,
    tools: response.tools?.map(item => ({
      id: item.id,
      name: item.name,
      state: item.state,
      error: item.error,
    })),
  };
  assert.equal(response.state, 'completed', `Agent turn: ${JSON.stringify(response.error)}`);
  assert(response.tools?.some(item => item.name === 'cube_browser_browse' && item.state === 'completed'),
    'The JCode browser tool event must complete');
  assert.equal(response.tools.length, 1, 'Exactly one published tool must be called');
  const sessionId = (await api(`/cases/${caseId}/conversation`)).session_id;
  report.session_id = sessionId;
  assert(sessionId, 'A completed Agent turn must have a durable session');

  const browserToken = await fetch(`${origin}/internal/agent-sessions/${sessionId}/cube/browser-browse`, {
    method: 'POST',
    headers: { 'content-type': 'application/json', authorization: `Bearer ${token}` },
    body: JSON.stringify({ tool_call_id: 'browser_token_probe', url: 'https://example.com' }),
  });
  assert(browserToken.status >= 400 && browserToken.status < 500,
    'A browser token must not authorize a Harness tool');
  report.browser_token_status = browserToken.status;

  const call = (await db.query(
    'SELECT id,tool_call_id,template_id,sandbox_id,state,result,error_code,cleanup_confirmed FROM cube_browser_tool_calls WHERE case_id=$1 AND message_id=$2',
    [caseId, messageId],
  )).rows;
  assert.equal(call.length, 1, 'Exactly one durable browser operation is required');
  const operation = call[0];
  assert.equal(operation.template_id, browserTemplate);
  assert.equal(operation.state, 'completed');
  assert.equal(operation.error_code, null);
  assert.equal(operation.cleanup_confirmed, true);
  assert.equal(operation.result.template_id, browserTemplate);
  assert.equal(operation.result.title, 'Example Domain');
  assert.equal(operation.result.heading, 'Example Domain');
  assert(operation.result.excerpt.includes('documentation examples'));
  assert.equal(operation.result.cleanup_confirmed, true);
  assert(operation.sandbox_id);
  const afterRejected = (await db.query(
    'SELECT count(*)::int AS count FROM cube_browser_tool_calls WHERE case_id=$1',
    [caseId],
  )).rows[0].count;
  assert.equal(afterRejected, before + 1, 'Rejected requests must not create another browser operation');
  report.rejected_requests_created_no_operations = true;
  report.operation = operation;
  const audit = (await db.query(
    "SELECT count(*)::int AS count FROM audit_events WHERE case_id=$1 AND event_type='agent.cube_browser_completed' AND payload->>'operation_id'=$2",
    [caseId, operation.id],
  )).rows[0].count;
  assert.equal(audit, 1, 'Exactly one durable browser completion audit is required');
  report.completion_audit_count = audit;

  const inventory = await cube('/sandboxes?limit=1000');
  assert(!inventory.some(item => item.sandboxID === operation.sandbox_id),
    'Browser tool instance must be absent after cleanup');
  report.tool_instance_absent = true;
  report.run = (await db.query(
    "SELECT r.state,r.timings,round(extract(epoch from r.finished_at-r.started_at)*1000)::int AS run_ms FROM agent_runs r WHERE r.message_id=$1",
    [messageId],
  )).rows[0];
  assert.equal(report.run.state, 'completed');
  assert(report.run.timings.at(-1)?.backend === 'cube', 'The Agent run must use Cube');
  passed = true;
} catch (error) {
  report.failure = error.message;
  process.exitCode = 1;
} finally {
  report.passed = passed;
  report.finished_at = new Date().toISOString();
  await writeFile(`${output}/report.json`, JSON.stringify(report, null, 2));
  await db.end();
}
console.log(JSON.stringify({
  passed: report.passed,
  failure: report.failure,
  case_id: caseId,
  report: `${output}/report.json`,
}, null, 2));
