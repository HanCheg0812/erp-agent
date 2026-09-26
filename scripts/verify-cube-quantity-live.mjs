import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { mkdir, writeFile } from 'node:fs/promises';
import pg from 'pg';

const origin = process.env.VERIFY_API_ORIGIN ?? 'http://127.0.0.1:3100';
const output = process.env.VERIFY_QUANTITY_OUTPUT ?? `/tmp/erp-cube-quantity/${Date.now()}`;
const residentTemplate = process.env.VERIFY_QUANTITY_RESIDENT_TEMPLATE;
const candidateId = process.env.VERIFY_QUANTITY_CANDIDATE_ID;
const toolTemplate = process.env.CUBE_QUANTITY_TEMPLATE_ID;
assert(residentTemplate && toolTemplate, 'Both resident and tool template IDs are required');

const db = new pg.Client({ connectionString: process.env.DATABASE_URL });
const report = { started_at: new Date().toISOString(), resident_template: residentTemplate, tool_template: toolTemplate };
let token;
let caseId;
let passed = false;
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

async function api(path, method = 'GET', body) {
  const response = await fetch(`${origin}/v1${path}`, {
    method,
    headers: { 'content-type': 'application/json', ...(token ? { authorization: `Bearer ${token}` } : {}) },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });
  const data = await response.json();
  assert(response.ok, `${method} ${path}: ${response.status} ${JSON.stringify(data)}`);
  return data;
}

async function cube(path) {
  const response = await fetch(`${process.env.CUBE_API_URL.replace(/\/$/, '')}${path}`, {
    headers: process.env.CUBE_API_KEY ? { 'X-API-Key': process.env.CUBE_API_KEY } : {},
  });
  assert.equal(response.status, 200, `Cube ${path}: ${response.status}`);
  return response.json();
}

async function releaseState() {
  const present = (await db.query("SELECT to_regclass('runtime_control') IS NOT NULL AS present")).rows[0].present;
  if (!present) return null;
  const row = (await db.query('SELECT active_release_id,version FROM runtime_control')).rows[0];
  return row ? { id: row.active_release_id, version: row.version } : null;
}

await mkdir(output, { recursive: true });
await db.connect();
try {
  token = (await api('/auth/login', 'POST', {
    email: process.env.DEMO_EMAIL, password: process.env.DEMO_PASSWORD,
  })).access_token;
  report.active_before = await releaseState();
  report.candidate_id = candidateId ?? null;
  caseId = candidateId
    ? (await api(`/runtime/candidates/${candidateId}/canary`, 'POST')).case_id
    : (await api('/conversations', 'POST', { request_id: randomUUID() })).id;
  report.case_id = caseId;

  const bogus = await fetch(`${origin}/internal/agent-sessions/unknown/cube/quantity-summary`, {
    method: 'POST', headers: { 'content-type': 'application/json', authorization: 'Bearer invalid' },
    body: JSON.stringify({ tool_call_id: 'unauthorized_probe', quantities: [12, 18, 20] }),
  });
  assert.equal(bogus.status, 401, 'A forged session must not run the tool');
  report.unauthorized_status = bogus.status;

  const submitted = await api(`/cases/${caseId}/conversation/messages`, 'POST', {
    client_message_id: randomUUID(),
    settings: { approval: 'ask', model: 'deepseek-flash', effort: 'low' },
    text: 'Synthetic test. Call cube_quantity_summary exactly once with quantities [12, 18, 20]. Report the total. Do not call ERP tools.',
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
  assert(response, 'Agent turn timed out');
  report.turn = { state: response.state, text: response.text, error: response.error,
    tools: response.tools?.map(item => ({ name: item.name, state: item.state, error: item.error })) };
  assert.equal(response.state, 'completed', `Agent turn: ${JSON.stringify(response.error)}`);
  assert(response.tools?.some(item => item.name === 'cube_quantity_summary' && item.state === 'completed'),
    'The JCode tool event must complete');

  const storedBinding = (await db.query(
    'SELECT to_jsonb(s) AS binding FROM case_sandboxes s WHERE case_id=$1 ORDER BY created_at DESC LIMIT 1',
    [caseId],
  )).rows[0]?.binding;
  const binding = storedBinding && {
    sandbox_id: storedBinding.last_sandbox_id ?? storedBinding.sandbox_id,
    template_id: storedBinding.runtime_template_id ?? null,
    candidate_id: storedBinding.runtime_candidate_id ?? null,
    release_id: storedBinding.runtime_release_id ?? null,
  };
  assert(binding?.sandbox_id);
  if (candidateId) {
    assert.equal(binding.template_id, residentTemplate);
    assert.equal(binding.candidate_id, candidateId);
    assert.equal(binding.release_id, null);
  }
  report.binding = binding;
  assert.equal((await cube(`/sandboxes/${binding.sandbox_id}`)).templateID, residentTemplate);
  report.run = (await db.query(
    "SELECT state,to_jsonb(r)->'timings' AS timings FROM agent_runs r WHERE message_id=$1",
    [messageId],
  )).rows[0];

  const calls = (await db.query(
    'SELECT id,template_id,sandbox_id,state,result,error_code,cleanup_confirmed FROM cube_quantity_tool_calls WHERE case_id=$1 AND message_id=$2',
    [caseId, messageId],
  )).rows;
  assert.equal(calls.length, 1);
  const call = calls[0];
  assert.equal(call.template_id, toolTemplate);
  assert.equal(call.state, 'completed');
  assert.equal(call.result.total, 50);
  assert.equal(call.result.count, 3);
  assert.equal(call.result.minimum, 12);
  assert.equal(call.result.maximum, 20);
  assert.equal(call.cleanup_confirmed, true);
  assert(call.sandbox_id && call.sandbox_id !== binding.sandbox_id);
  report.tool_call = call;
  const audit = (await db.query(
    "SELECT count(*)::int AS count FROM audit_events WHERE case_id=$1 AND event_type='agent.cube_tool_completed' AND payload->>'operation_id'=$2",
    [caseId, call.id],
  )).rows[0];
  assert.equal(audit.count, 1, 'Exactly one durable completion audit is required');
  report.completion_audit_count = audit.count;
  const inventory = await cube('/sandboxes?limit=1000');
  assert(!inventory.some(item => item.sandboxID === call.sandbox_id), 'Tool instance must be absent after cleanup');
  report.tool_instance_absent = true;

  report.active_after = await releaseState();
  assert.deepEqual(report.active_after, report.active_before, 'Canary must not activate a release');
  passed = true;
} catch (error) {
  report.failure = error.message;
  process.exitCode = 1;
} finally {
  if (caseId) {
    try {
      const history = await api(`/cases/${caseId}/conversation`);
      for (const item of history.items.filter(item => ['queued', 'running', 'waiting'].includes(item.response?.state))) {
        await api(`/cases/${caseId}/conversation/cancel`, 'POST', { message_id: String(item.id) });
      }
      const state = await api(`/cases/${caseId}/sandbox`);
      if (state.operation && state.operation.state !== 'released') {
        const released = await api(`/cases/${caseId}/sandbox/${state.operation.id}`, 'DELETE');
        report.resident_cleanup = released.operation?.state;
        assert.equal(report.resident_cleanup, 'released');
      }
      if (report.binding?.sandbox_id) {
        const inventory = await cube('/sandboxes?limit=1000');
        report.resident_instance_absent = !inventory.some(item => item.sandboxID === report.binding.sandbox_id);
        assert(report.resident_instance_absent, 'Resident instance must be absent after cleanup');
      }
    } catch (error) {
      report.cleanup_failure = error.message;
      passed = false;
      process.exitCode = 1;
    }
  }
  report.passed = passed;
  report.finished_at = new Date().toISOString();
  await writeFile(`${output}/report.json`, JSON.stringify(report, null, 2));
  await db.end();
}
console.log(JSON.stringify({ passed: report.passed, failure: report.failure,
  cleanup_failure: report.cleanup_failure, case_id: caseId, report: `${output}/report.json` }, null, 2));
