import assert from 'node:assert/strict';
import { createHmac, randomUUID } from 'node:crypto';
import { mkdir, writeFile } from 'node:fs/promises';
import pg from 'pg';

const origin = process.env.VERIFY_API_ORIGIN ?? 'http://127.0.0.1:3100';
const backend = process.env.VERIFY_RUNTIME_BACKEND ?? 'cube';
assert(['cube', 'host'].includes(backend), 'VERIFY_RUNTIME_BACKEND must be cube or host');
const count = Number(process.env.VERIFY_CUBE_COUNT ?? 8);
const timeoutMs = Number(process.env.VERIFY_CUBE_TIMEOUT_MS ?? 300000);
const output = process.env.VERIFY_CUBE_OUTPUT ?? `/tmp/erp-cube-acceptance/${Date.now()}`;
const syntheticContextKiB = Number(process.env.VERIFY_CUBE_SYNTHETIC_CONTEXT_KIB ?? 0);
const syntheticRounds = Number(process.env.VERIFY_CUBE_SYNTHETIC_ROUNDS ?? 2);
assert(Number.isInteger(syntheticContextKiB) && syntheticContextKiB >= 0 && syntheticContextKiB <= 3,
  'VERIFY_CUBE_SYNTHETIC_CONTEXT_KIB must be 0..3 to respect the 4000-character message limit');
assert(Number.isInteger(syntheticRounds) && syntheticRounds >= 2 && syntheticRounds <= 8,
  'VERIFY_CUBE_SYNTHETIC_ROUNDS must be 2..8');
const syntheticRecords = [];
let syntheticBytes = 0;
while (syntheticBytes < syntheticContextKiB * 1024) {
  const index = syntheticRecords.length + 1;
  const line = `Synthetic order record ${String(index).padStart(4, '0')}: part=P-${String(index).padStart(4, '0')}, quantity=${100 + index}, revision=A, status=planned, source=load-test.`;
  syntheticRecords.push(line);
  syntheticBytes += Buffer.byteLength(line) + 1;
}
const syntheticPrompt = syntheticRecords.length
  ? `以下都是合成压测数据。不要调用工具。请只回答最后一条记录的 part 和 quantity。\n${syntheticRecords.join('\n')}`
  : null;
const matchedPrompt = process.env.VERIFY_CUBE_MATCHED_PROMPT?.trim() || syntheticPrompt;
assert(!matchedPrompt || [...matchedPrompt].length <= 4000, 'Matched prompt exceeds the 4000-character message limit');
const expectNoTools = process.env.VERIFY_EXPECT_NO_TOOLS === '1';
const expectedAnswer = process.env.VERIFY_EXPECTED_ANSWER?.trim();
assert(!expectedAnswer || matchedPrompt, 'VERIFY_EXPECTED_ANSWER requires a matched prompt');
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
const stageSummary = (messages, name) => {
  const values = messages.map(item => item.stages_ms?.[name]).filter(Number.isFinite);
  return { p50_ms: percentile(values, 0.5), p95_ms: percentile(values, 0.95), samples: values.length };
};
const answerMatches = text => {
  if (!expectedAnswer || typeof text !== 'string') return false;
  if (/^\d+$/.test(expectedAnswer)) return (text.match(/\d+/g) ?? []).at(-1) === expectedAnswer;
  return text.includes(expectedAnswer);
};

function stages(timing) {
  if (!timing) return null;
  const offset = timing.offsets_ms ?? {};
  const lifecycle = timing.lifecycle ?? {};
  const runtime = timing.runtime ?? {};
  const difference = (end, start) => Number.isFinite(offset[end]) && Number.isFinite(offset[start])
    ? offset[end] - offset[start] : null;
  return {
    sandbox_ready: offset.sandbox_ready ?? null,
    runtime_started: offset.runtime_started ?? null,
    sandbox_status: lifecycle.status_ms ?? null,
    cube_create_request: lifecycle.create_request_ms ?? null,
    guest_configuration: lifecycle.configure_ms ?? null,
    cube_create_and_configure: lifecycle.create_operation_ms ?? null,
    input_preparation: difference('input_ready', timing.backend === 'host' ? 'runtime_started' : 'sandbox_ready'),
    bridge_headers: difference('bridge_response', 'bridge_request'),
    bridge_to_catalog: difference('catalog', 'bridge_request'),
    runtime_to_catalog: difference('catalog', 'runtime_started'),
    jcode_build: runtime.build_ms ?? null,
    model_catalog: runtime.model_catalog_ms ?? null,
    catalog_to_first_reasoning: difference('first_reasoning', 'catalog'),
    catalog_to_first_text: difference('first_text', 'catalog'),
    first_reasoning: offset.first_reasoning ?? null,
    first_text: offset.first_text ?? null,
    tool_time_total: (timing.tools ?? []).reduce((total, tool) => total + tool.duration_ms, 0),
    turn_elapsed: timing.elapsed_ms,
  };
}

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

async function verifyCubeTemplate() {
  const origin = process.env.CUBE_API_URL?.replace(/\/$/, '');
  const expected = process.env.CUBE_TEMPLATE_ID;
  assert(origin && expected, 'Cube API URL and template ID are required for the template probe');
  const response = await fetch(`${origin}/sandboxes?limit=1000`);
  assert.equal(response.status, 200, 'Cube sandbox inventory must be reachable');
  const instances = await response.json();
  assert(Array.isArray(instances), 'Cube sandbox inventory must be an array');
  const byId = new Map(instances.map(instance => [instance.sandboxID, instance]));
  return cases.map(entry => {
    const instance = byId.get(entry.sandbox_id);
    assert(instance, `Cube sandbox ${entry.sandbox_id} is missing from inventory`);
    assert.equal(instance.templateID, expected, `Cube sandbox ${entry.sandbox_id} used an unexpected template`);
    return {
      sandbox_id: entry.sandbox_id,
      template_id: instance.templateID,
      cpu_milli: instance.cpuMilli,
      memory_mb: instance.memoryMB,
    };
  });
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
        if (expectedAnswer) message.answer_correct = answerMatches(response.text);
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
        r.state, r.error, r.timings
      FROM agent_runs r JOIN case_conversation_messages m ON m.id=r.message_id
      WHERE r.message_id::text=ANY($1::text[])`, [ids]);
    for (const timing of timings.rows) {
      const message = messages.find(item => item.message_id === timing.message_id);
      Object.assign(message, timing);
      message.stages_ms = stages(timing.timings?.at(-1));
      message.runtime_backend = timing.timings?.at(-1)?.backend ?? null;
      message.runtime_reused = timing.timings?.at(-1)?.runtime?.reused ?? null;
    }
  }
  return {
    name, submitted_at: new Date(submittedAt).toISOString(), messages,
    summary: {
      completed: messages.filter(item => item.state === 'completed').length,
      failed: messages.filter(item => item.state !== 'completed').length,
      ...(expectedAnswer ? {
        correct: messages.filter(item => item.state === 'completed' && item.answer_correct).length,
        incorrect: messages.filter(item => item.state === 'completed' && !item.answer_correct).length,
      } : {}),
      peak_running: Math.max(0, ...samples.filter(item => item.round === name).map(item => item.running ?? 0)),
      queue_p50_ms: percentile(messages.map(item => item.queue_ms).filter(Number.isFinite), 0.5),
      queue_p95_ms: percentile(messages.map(item => item.queue_ms).filter(Number.isFinite), 0.95),
      first_token_p50_ms: percentile(messages.map(item => item.first_token_after_submit_ms).filter(Number.isFinite), 0.5),
      first_token_p95_ms: percentile(messages.map(item => item.first_token_after_submit_ms).filter(Number.isFinite), 0.95),
      total_p50_ms: percentile(messages.map(item => item.total_ms).filter(Number.isFinite), 0.5),
      total_p95_ms: percentile(messages.map(item => item.total_ms).filter(Number.isFinite), 0.95),
      stages: Object.fromEntries([
        'sandbox_ready', 'runtime_started', 'sandbox_status', 'cube_create_request', 'guest_configuration',
        'cube_create_and_configure', 'input_preparation', 'bridge_headers',
        'bridge_to_catalog', 'runtime_to_catalog', 'jcode_build', 'model_catalog',
        'catalog_to_first_reasoning', 'catalog_to_first_text', 'first_reasoning',
        'first_text', 'tool_time_total', 'turn_elapsed',
      ].map(stage => [stage, stageSummary(messages, stage)])),
      tool_counts: Object.fromEntries([...new Set(messages.map(item => item.tool_names?.length ?? 0))]
        .map(n => [n, messages.filter(item => (item.tool_names?.length ?? 0) === n).length])),
    },
  };
}

await db.connect();
const report = {
  count, origin, backend, rounds: [], cases, samples, cleanup: [],
  workload: matchedPrompt
    ? { same_prompt: true, expect_no_tools: expectNoTools, synthetic_context_kib: syntheticContextKiB,
        rounds: syntheticContextKiB ? syntheticRounds : 2,
        ...(expectedAnswer ? { expected_answer: expectedAnswer } : {}),
        prompt_bytes: Buffer.byteLength(matchedPrompt), synthetic_last_part: syntheticRecords.length
          ? `P-${String(syntheticRecords.length).padStart(4, '0')}` : null,
        note: 'The warm turn has prior conversation history; context and output are not identical.' }
    : { same_prompt: false, expected_cold_tools: ['mcp__erp__list_erp_orders', 'mcp__erp__read_erp_order'], expected_warm_tools: 0 },
};
try {
  token = (await api('/auth/login', 'POST', {
    email: process.env.DEMO_EMAIL, password: process.env.DEMO_PASSWORD,
  })).access_token;
  const created = await Promise.all(Array.from({ length: count }, () =>
    api('/conversations', 'POST', { request_id: randomUUID() })));
  for (const item of created) cases.push({ id: item.id });
  report.rounds.push(await runRound('cold', matchedPrompt ?? '请使用 ERP 只读工具列出我有权限的订单，并读取列表中的第一张订单。只需回答真实订单号和数量，不修改业务数据。'));
  if (report.rounds[0].summary.completed === count) {
    for (const entry of cases) {
      if (backend === 'cube') {
        const state = await api(`/cases/${entry.id}/sandbox`);
        entry.operation = state.operation?.id;
        entry.sandbox_id = state.operation?.sandbox_id;
      }
      entry.session_id = (await api(`/cases/${entry.id}/conversation`)).session_id;
    }
    if (backend === 'cube') report.instances = await verifyCubeTemplate();
    if (backend === 'cube' && cases.length > 1) report.authorization = await verifyCaseTokenIsolation(cases[0], cases[1]);
    const totalRounds = syntheticContextKiB ? syntheticRounds : 2;
    for (let index = 1; index < totalRounds; index++) {
      report.rounds.push(await runRound(index === 1 ? 'warm' : `warm-${index + 1}`,
        matchedPrompt ?? '请复述刚才查询到的 ERP 订单号和数量。不要再次调用工具。'));
      if (report.rounds.at(-1).summary.completed !== count) break;
    }
  }
  const bindings = await db.query(
    "SELECT case_id::text,session_id,sandbox_id,state FROM case_sandboxes WHERE case_id=ANY($1::uuid[]) AND state<>'released'",
    [cases.map(entry => entry.id)],
  );
  report.bindings = bindings.rows;
  report.isolated = new Set(cases.map(entry => entry.session_id)).size === count &&
    (backend === 'cube'
      ? bindings.rows.length === count &&
        new Set(bindings.rows.map(row => row.sandbox_id)).size === count &&
        new Set(bindings.rows.map(row => row.session_id)).size === count
      : bindings.rows.length === 0);
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
assert(report.rounds.every(round => round.summary.completed === count), 'Some Agent turns did not complete');
assert(report.rounds.every(round => round.messages.every(message => message.runtime_backend === backend)), 'Recorded runtime backend did not match the requested backend');
assert(report.rounds.every(round => round.messages.every(message =>
  (message.tool_names ?? []).every(tool => tool.endsWith(':completed')))), 'An Agent tool did not complete');
if (!matchedPrompt) {
  for (const message of report.rounds[0].messages) {
    for (const name of report.workload.expected_cold_tools) {
      assert(message.tool_names?.includes(`${name}:completed`), `Cold ERP turn did not complete ${name}`);
    }
  }
  assert(report.rounds[1].messages.every(message => message.tool_names?.length === 0), 'Warm ERP turn called a tool');
}
if (expectNoTools) {
  assert(report.rounds.every(round => round.messages.every(message => message.tool_names?.length === 0)),
    'A no-tool workload called a tool');
}
if (expectedAnswer) {
  assert(report.rounds.every(round => round.summary.correct === count), 'Some completed Agent replies did not match the expected answer');
}
assert(report.isolated, 'Case sandboxes or sessions were not isolated');
