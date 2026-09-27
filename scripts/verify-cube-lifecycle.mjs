import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { mkdir, writeFile } from 'node:fs/promises';
import pg from 'pg';

const origin = process.env.VERIFY_API_ORIGIN ?? 'http://127.0.0.1:3100';
const cubeOrigin = (process.env.CUBE_API_URL ?? 'http://127.0.0.1:3001').replace(/\/$/, '');
const timeoutMs = Number(process.env.VERIFY_CUBE_TIMEOUT_MS ?? 180000);
const skipPause = process.env.VERIFY_CUBE_SKIP_PAUSE === '1';
const pauseCycles = skipPause ? 0 : Number(process.env.VERIFY_CUBE_PAUSE_CYCLES ?? 1);
assert(Number.isInteger(pauseCycles) && pauseCycles >= 0 && pauseCycles <= 5);
const sameCaseTurns = Number(process.env.VERIFY_CUBE_SAME_CASE_TURNS ?? 0);
assert(Number.isInteger(sameCaseTurns) && sameCaseTurns >= 0 && sameCaseTurns <= 5);
const keepFailed = process.env.VERIFY_CUBE_KEEP_FAILED === '1';
const resumeDelayMs = Number(process.env.VERIFY_CUBE_RESUME_DELAY_MS ?? 0);
const output = process.env.VERIFY_CUBE_OUTPUT ?? `/tmp/erp-cube-acceptance/lifecycle-${Date.now()}`;
const matchedPrompt = process.env.VERIFY_CUBE_MATCHED_PROMPT?.trim();
const db = new pg.Client({ connectionString: process.env.DATABASE_URL });
let token;
let caseId;
let operationId;
let releasedOperationId;

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

async function cube(path, method = 'GET', body) {
  const response = await fetch(`${cubeOrigin}${path}`, {
    method,
    headers: { 'content-type': 'application/json', ...(process.env.CUBE_API_KEY ? { 'x-api-key': process.env.CUBE_API_KEY } : {}) },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });
  const text = await response.text();
  const data = text ? JSON.parse(text) : null;
  if (!response.ok) throw new Error(`Cube ${method} ${path}: ${response.status} ${JSON.stringify(data)}`);
  return data;
}

const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

async function waitForResponse(messageId) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const history = await api(`/cases/${caseId}/conversation`);
    const message = history.items.find(item => item.id === messageId);
    if (message?.response && ['completed', 'failed', 'cancelled'].includes(message.response.state)) return message.response;
    await sleep(500);
  }
  throw new Error(`Timed out waiting for response ${messageId}`);
}

async function send(text) {
  const result = await api(`/cases/${caseId}/conversation/messages`, 'POST', {
    client_message_id: randomUUID(),
    settings: { approval: 'ask', model: 'deepseek-flash', effort: 'low' },
    text,
  });
  const response = await waitForResponse(result.message.id);
  const timing = (await db.query(`
    SELECT r.state,
      round(extract(epoch from r.started_at - m.created_at)*1000)::int AS queue_ms,
      round(extract(epoch from r.first_token_at - m.created_at)*1000)::int AS first_text_after_submit_ms,
      round(extract(epoch from r.finished_at - m.created_at)*1000)::int AS total_ms,
      r.timings
    FROM agent_runs r JOIN case_conversation_messages m ON m.id=r.message_id
    WHERE r.message_id=$1`, [result.message.id])).rows[0];
  assert(timing, `Missing agent_run timing for ${result.message.id}`);
  return { message_id: result.message.id, response, timing: {
    queue_ms: timing.queue_ms,
    first_text_after_submit_ms: timing.first_text_after_submit_ms,
    total_ms: timing.total_ms,
    ...(timing.timings?.at(-1) ?? {}),
  } };
}

async function sandboxStatus() {
  return api(`/cases/${caseId}/sandbox`);
}

async function waitForSandbox(predicate) {
  const deadline = Date.now() + timeoutMs;
  let current;
  while (Date.now() < deadline) {
    current = await sandboxStatus();
    if (predicate(current)) return current;
    await sleep(500);
  }
  throw new Error(`Timed out waiting for sandbox state: ${JSON.stringify(current)}`);
}

const report = {
  origin, cube_origin: cubeOrigin, case_id: null, steps: [], errors: [],
  workload: matchedPrompt
    ? { same_prompt: true, note: 'Later turns include prior conversation history.' }
    : { same_prompt: false },
};
await db.connect();
try {
  token = (await api('/auth/login', 'POST', {
    email: process.env.DEMO_EMAIL, password: process.env.DEMO_PASSWORD,
  })).access_token;
  const conversation = await api('/conversations', 'POST', { request_id: randomUUID() });
  caseId = conversation.id;
  report.case_id = caseId;

  const first = await send(matchedPrompt ?? '请使用 ERP 只读工具读取一张订单，然后只回答订单号和数量，不修改业务数据。');
  assert.equal(first.response.state, 'completed', JSON.stringify(first.response));
  let status = await waitForSandbox(value => value.operation?.state === 'provisioned' && value.operation?.guest_ready === true);
  operationId = status.operation.id;
  const firstSandboxId = status.operation.sandbox_id;
  const firstSession = (await db.query(
    'SELECT session_id FROM case_sandboxes WHERE id=$1', [operationId],
  )).rows[0]?.session_id;
  report.steps.push({ name: 'initial_turn', state: first.response.state, message_id: first.message_id, timing: first.timing, operation_id: operationId, sandbox_id: firstSandboxId, session_id: firstSession, tools: first.response.tools?.map(tool => tool.name) });

  for (let cycle = 1; cycle <= pauseCycles; cycle += 1) {
    const beforePause = await cube(`/sandboxes/${firstSandboxId}`);
    await cube(`/sandboxes/${firstSandboxId}/pause`, 'POST', {});
    const paused = await (async () => {
      const deadline = Date.now() + 60000;
      let value;
      while (Date.now() < deadline) {
        value = await cube(`/sandboxes/${firstSandboxId}`);
        if (String(value?.state).toLowerCase() === 'paused') return value;
        await sleep(500);
      }
      throw new Error(`Cube did not pause: ${JSON.stringify(value)}`);
    })();
    report.steps.push({ name: 'paused', cycle, before_state: beforePause?.state, state: paused?.state, sandbox_id: firstSandboxId });
    if (resumeDelayMs > 0) {
      await waitForSandbox(value => value.operation?.state === 'provisioned' && value.operation?.guest_ready === true);
      await sleep(resumeDelayMs);
    }

    const resumed = await send(matchedPrompt ?? `第 ${cycle} 次恢复后，请简短复述最初查询的订单号和数量，不要再次调用工具。`);
    assert.equal(resumed.response.state, 'completed', JSON.stringify(resumed.response));
    status = await waitForSandbox(value => value.operation?.state === 'provisioned' && value.operation?.guest_ready === true);
    assert.equal(status.operation.id, operationId, 'Pause/resume created a new operation');
    assert.equal(status.operation.sandbox_id, firstSandboxId, 'Pause/resume created a new sandbox');
    const resumedSession = (await db.query(
      'SELECT session_id FROM case_sandboxes WHERE id=$1', [operationId],
    )).rows[0]?.session_id;
    assert.equal(resumedSession, firstSession, 'Pause/resume changed the PostgreSQL session');
    const afterResume = await cube(`/sandboxes/${firstSandboxId}`);
    report.steps.push({ name: 'pause_resume_turn', cycle, state: resumed.response.state, message_id: resumed.message_id, timing: resumed.timing, cube_state: afterResume?.state, operation_id: status.operation.id, sandbox_id: status.operation.sandbox_id, session_id: resumedSession, tools: resumed.response.tools?.map(tool => tool.name), text_excerpt: resumed.response.text?.slice(0, 220) });
  }

  if (sameCaseTurns > 0) {
    const clientMessageId = randomUUID();
    const body = {
      client_message_id: clientMessageId,
      settings: { approval: 'ask', model: 'deepseek-flash', effort: 'low' },
      text: '同案提交验证：请只回复“收到第一条”，不要调用工具。',
    };
    const submitted = await Promise.all(Array.from({ length: 2 }, () =>
      api(`/cases/${caseId}/conversation/messages`, 'POST', body)));
    assert.equal(submitted[0].message.id, submitted[1].message.id, 'Duplicate request created two messages');
    assert.equal(submitted.filter(item => item.replayed === true).length, 1, 'Duplicate request was not replayed');
    const busy = await fetch(`${origin}/v1/cases/${caseId}/conversation/messages`, {
      method: 'POST',
      headers: { 'content-type': 'application/json', authorization: `Bearer ${token}` },
      body: JSON.stringify({ ...body, client_message_id: randomUUID() }),
    });
    assert.equal(busy.status, 409, 'A different request was accepted during an active turn');
    assert.equal((await busy.json()).error, 'agent_busy');
    const firstFollowup = await waitForResponse(submitted[0].message.id);
    assert.equal(firstFollowup.state, 'completed');
    const followups = [{ message_id: submitted[0].message.id, response: firstFollowup }];
    for (let index = 1; index < sameCaseTurns; index += 1) {
      const result = await send(`同案续轮 ${index + 1}：请只回复“收到 ${index + 1}”，不要调用工具。`);
      assert.equal(result.response.state, 'completed');
      followups.push(result);
    }
    const timings = (await db.query(
      "SELECT message_id::text,started_at,finished_at,state FROM agent_runs WHERE message_id::text=ANY($1::text[]) ORDER BY started_at",
      [followups.map(item => String(item.message_id))],
    )).rows;
    assert.equal(timings.length, sameCaseTurns);
    assert(timings.every(item => item.state === 'completed' && item.started_at && item.finished_at));
    assert(timings.every((item, index) => index === 0 || timings[index - 1].finished_at <= item.started_at), 'Same-case turns overlapped');
    status = await waitForSandbox(value => value.operation?.state === 'provisioned' && value.operation?.guest_ready === true);
    assert.equal(status.operation.id, operationId, 'Same-case turns changed the operation');
    assert.equal(status.operation.sandbox_id, firstSandboxId, 'Same-case turns changed the sandbox');
    report.steps.push({ name: 'same_case_turns', count: sameCaseTurns, completed: followups.length, duplicate_replayed: true, concurrent_new_request_status: busy.status, serialized: true, operation_id: operationId, sandbox_id: firstSandboxId, timings: timings.map(item => ({ message_id: item.message_id, started_at: item.started_at, finished_at: item.finished_at })) });
  }

  const released = await api(`/cases/${caseId}/sandbox/${operationId}`, 'DELETE');
  releasedOperationId = operationId;
  report.steps.push({ name: 'release_requested', state: released.operation?.state, operation_id: operationId });
  await waitForSandbox(value => value.operation?.state === 'released');
  const recreated = await send(matchedPrompt ?? '请只回答“会话已恢复”，不要调用工具。');
  assert.equal(recreated.response.state, 'completed', JSON.stringify(recreated.response));
  status = await waitForSandbox(value => value.operation?.state === 'provisioned' && value.operation?.guest_ready === true);
  const secondOperationId = status.operation.id;
  const secondSession = (await db.query(
    'SELECT session_id FROM case_sandboxes WHERE id=$1', [secondOperationId],
  )).rows[0]?.session_id;
  report.steps.push({ name: 'recreated_turn', state: recreated.response.state, message_id: recreated.message_id, timing: recreated.timing, operation_id: secondOperationId, sandbox_id: status.operation.sandbox_id, session_id: secondSession, same_session: firstSession === secondSession, new_sandbox: firstSandboxId !== status.operation.sandbox_id });
  operationId = secondOperationId;
} catch (error) {
  report.errors.push(String(error));
} finally {
  if (caseId && operationId && (!keepFailed || report.errors.length === 0)) {
    try {
      const current = await sandboxStatus();
      if (current.operation && current.operation.state !== 'released') {
        await api(`/cases/${caseId}/sandbox/${operationId}`, 'DELETE');
      }
    } catch (error) { report.errors.push(`cleanup: ${error}`); }
  }
  await mkdir(output, { recursive: true });
  await writeFile(`${output}/report.json`, JSON.stringify(report, null, 2));
  console.log(JSON.stringify({ report: `${output}/report.json`, steps: report.steps, errors: report.errors }));
  await db.end();
}
assert.equal(report.errors.length, 0, report.errors.join('\n'));
assert.equal(report.steps.filter(step => step.name === 'pause_resume_turn' && step.state === 'completed').length, pauseCycles);
if (sameCaseTurns > 0) assert.equal(report.steps.find(step => step.name === 'same_case_turns')?.completed, sameCaseTurns);
assert.equal(report.steps.find(step => step.name === 'recreated_turn')?.state, 'completed');
