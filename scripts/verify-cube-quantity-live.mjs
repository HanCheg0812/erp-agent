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
const rounds = Number(process.env.VERIFY_QUANTITY_ROUNDS ?? 1);
const recreateAfter = Number(process.env.VERIFY_QUANTITY_RECREATE_AFTER ?? 0);
assert(Number.isInteger(rounds) && rounds >= 1 && rounds <= 8, 'Rounds must be 1..8');
assert(Number.isInteger(recreateAfter) && recreateAfter >= 0 && recreateAfter < rounds,
  'Recreation must precede a subsequent round');
const marker = `quantity-${randomUUID()}`;
const attempt = randomUUID();
const inputs = [[12, 18, 20], [0, 7, 11], Array.from({ length: 32 }, (_, i) => i === 31 ? 1_000_000 : i), [0]];

const db = new pg.Client({ connectionString: process.env.DATABASE_URL });
const report = { started_at: new Date().toISOString(), resident_template: residentTemplate,
  tool_template: toolTemplate, attempt, rounds: [], resident_instances: [] };
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

async function releaseResident() {
  const state = await api(`/cases/${caseId}/sandbox`);
  if (state.operation && state.operation.state !== 'released') {
    const released = await api(`/cases/${caseId}/sandbox/${state.operation.id}`, 'DELETE');
    assert.equal(released.operation?.state, 'released');
  }
  const inventory = await cube('/sandboxes?limit=1000');
  for (const id of report.resident_instances) {
    assert(!inventory.some(item => item.sandboxID === id), `Resident ${id} must be absent after release`);
  }
  return { state: 'released', instances_absent: true };
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
  const sessionId = (await api(`/cases/${caseId}/conversation`)).session_id;
  const operationsBefore = (await db.query(
    'SELECT count(*)::int AS count FROM cube_quantity_tool_calls WHERE case_id=$1', [caseId],
  )).rows[0].count;

  const bogus = await fetch(`${origin}/internal/agent-sessions/${sessionId}/cube/quantity-summary`, {
    method: 'POST', headers: { 'content-type': 'application/json', authorization: 'Bearer invalid' },
    body: JSON.stringify({ tool_call_id: 'unauthorized_probe', quantities: [12, 18, 20] }),
  });
  assert.equal(bogus.status, 401, 'A forged session must not run the tool');
  report.unauthorized_status = bogus.status;

  const browserToken = await fetch(`${origin}/internal/agent-sessions/${sessionId}/cube/quantity-summary`, {
    method: 'POST', headers: { 'content-type': 'application/json', authorization: `Bearer ${token}` },
    body: JSON.stringify({ tool_call_id: 'browser_token_probe', quantities: [12, 18, 20] }),
  });
  assert.equal(browserToken.status, 401, 'A browser token must not authorize a Harness tool');
  report.browser_token_status = browserToken.status;
  assert.equal((await db.query('SELECT count(*)::int AS count FROM cube_quantity_tool_calls WHERE case_id=$1',
    [caseId])).rows[0].count, operationsBefore, 'Rejected requests must not create a tool operation');
  report.rejected_requests_created_no_operations = true;

  for (let index = 0; index < rounds; index++) {
    const quantities = inputs[index % inputs.length];
    const expected = { total: quantities.reduce((sum, n) => sum + n, 0), count: quantities.length,
      minimum: Math.min(...quantities), maximum: Math.max(...quantities) };
    const round = { round: index + 1, quantities, expected };
    report.rounds.push(round);
    const submitted = await api(`/cases/${caseId}/conversation/messages`, 'POST', {
      client_message_id: randomUUID(),
      settings: { approval: 'ask', model: 'deepseek-flash', effort: 'low' },
      text: `Synthetic acceptance attempt ${attempt}, round ${index + 1}. Call cube_quantity_summary exactly once with quantities ${JSON.stringify(quantities)}. Do not call any other tools. ${index === 0 ? `Remember this marker for THIS attempt only: ${marker}. Earlier acceptance attempts have different markers.` : 'Recall the marker supplied for THIS acceptance attempt, including after instance recreation. Do not use markers from older acceptance attempts.'} Reply with MARKER=<remembered marker> and TOTAL=<total returned by the tool>.`,
    });
    const messageId = submitted.message.id;
    round.message_id = messageId;
    const deadline = Date.now() + 240_000;
    let response;
    while (Date.now() < deadline) {
      const history = await api(`/cases/${caseId}/conversation`);
      response = history.items.find(item => item.id === messageId)?.response;
      if (response && ['completed', 'failed', 'cancelled'].includes(response.state)) break;
      await sleep(1000);
    }
    assert(response, 'Agent turn timed out');
    round.turn = { state: response.state, text: response.text, error: response.error,
      tools: response.tools?.map(item => ({ name: item.name, state: item.state, error: item.error })) };
    assert.equal(response.state, 'completed', `Agent turn: ${JSON.stringify(response.error)}`);
    assert(response.tools?.some(item => item.name === 'cube_quantity_summary' && item.state === 'completed'),
      'The JCode tool event must complete');
    assert.equal(response.tools.length, 1, 'Exactly one published tool must be called');
    const plain = response.text.replaceAll('*', '').replaceAll('`', '');
    assert.equal(plain.match(/\bTOTAL\s*=\s*([\d,]+)/i)?.[1]?.replaceAll(',', ''), String(expected.total),
      'Final reply must contain the actual tool total');
    assert(plain.includes(`MARKER=${marker}`), 'The remembered marker must survive subsequent turns');
    round.answer_correct = true;
    const history = await api(`/cases/${caseId}/conversation`);
    round.session_id = history.session_id;
    if (index) assert.equal(round.session_id, report.rounds[0].session_id, 'JCode session must survive recreation');

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
    round.binding = binding;
    if (!report.resident_instances.includes(binding.sandbox_id)) report.resident_instances.push(binding.sandbox_id);
    if (index === recreateAfter && recreateAfter) {
      assert.notEqual(binding.sandbox_id, report.rounds[index - 1].binding.sandbox_id,
        'The recreated resident must have a new Cube instance ID');
    } else if (index) {
      assert.equal(binding.sandbox_id, report.rounds[index - 1].binding.sandbox_id,
        'Warm turns must reuse the resident instance');
    }
    assert.equal((await cube(`/sandboxes/${binding.sandbox_id}`)).templateID, residentTemplate);
    round.run = (await db.query(
      "SELECT r.state,r.timings,round(extract(epoch from r.started_at-m.created_at)*1000)::int AS queue_ms,round(extract(epoch from r.finished_at-m.created_at)*1000)::int AS total_ms FROM agent_runs r JOIN case_conversation_messages m ON m.id=r.message_id WHERE r.message_id=$1",
      [messageId],
    )).rows[0];
    assert.equal(round.run.state, 'completed');
    assert.equal(round.run.timings.at(-1)?.backend, 'cube', 'Host fallback must not count as acceptance');

    const calls = (await db.query(
      'SELECT id,template_id,sandbox_id,state,result,error_code,cleanup_confirmed FROM cube_quantity_tool_calls WHERE case_id=$1 AND message_id=$2',
      [caseId, messageId],
    )).rows;
    assert.equal(calls.length, 1);
    const call = calls[0];
    assert.equal(call.template_id, toolTemplate);
    assert.equal(call.state, 'completed');
    assert.deepEqual(Object.fromEntries(Object.keys(expected).map(key => [key, call.result[key]])), expected);
    assert.equal(call.result.template_id, call.template_id);
    assert.equal(call.result.sandbox_id, call.sandbox_id);
    assert.equal(call.result.cleanup_confirmed, true);
    assert.equal(call.cleanup_confirmed, true);
    assert(call.sandbox_id && call.sandbox_id !== binding.sandbox_id);
    round.tool_call = call;
    const audit = (await db.query(
      "SELECT count(*)::int AS count FROM audit_events WHERE case_id=$1 AND event_type='agent.cube_tool_completed' AND payload->>'operation_id'=$2",
      [caseId, call.id],
    )).rows[0];
    assert.equal(audit.count, 1, 'Exactly one durable completion audit is required');
    round.completion_audit_count = audit.count;
    const inventory = await cube('/sandboxes?limit=1000');
    assert(!inventory.some(item => item.sandboxID === call.sandbox_id), 'Tool instance must be absent after cleanup');
    round.tool_instance_absent = true;
    round.session_storage = (await db.query(
      "SELECT s.revision,(SELECT count(*)::int FROM agent_session_entries e JOIN agent_session_heads h USING(session_id,kind,generation) WHERE e.session_id=s.id AND e.kind='messages') AS active_entries FROM agent_sessions s WHERE s.id=$1",
      [round.session_id],
    )).rows[0];
    assert(round.session_storage.active_entries > 0, 'The transcript must be stored in PostgreSQL');
    console.log(JSON.stringify({ round: round.round, message_id: messageId, total: expected.total,
      state: response.state, sandbox_id: binding.sandbox_id, elapsed_ms: round.run.total_ms }));
    if (index + 1 === recreateAfter) report.recreation = await releaseResident();
  }

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
      report.resident_cleanup = await releaseResident();
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
