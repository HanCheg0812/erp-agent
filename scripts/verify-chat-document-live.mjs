// Explicit, synthetic acceptance of long attachment reading through the model.
import assert from 'node:assert/strict';
import { randomBytes, randomUUID } from 'node:crypto';

assert.equal(process.env.LIVE_ACCEPTANCE, '1', 'Set LIVE_ACCEPTANCE=1 to call the configured model');
const origin = process.env.LIVE_DOCUMENT_ORIGIN ?? 'http://127.0.0.1:3101';
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));

async function request(path, token, body) {
  const response = await fetch(`${origin}/v1${path}`, {
    method: body === undefined ? 'GET' : 'POST',
    headers: {
      ...(token ? { authorization: `Bearer ${token}` } : {}),
      ...(body === undefined ? {} : { 'content-type': 'application/json' }),
    },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });
  const result = await response.json();
  assert(response.ok, `${path}: ${response.status} ${JSON.stringify(result)}`);
  return result;
}

assert((await fetch(`${origin}/ready`)).ok, 'API is not ready');
const suffix = randomBytes(6).toString('hex');
const auth = await request('/auth/register', undefined, {
  email: `document-${suffix}@example.test`,
  password: randomBytes(24).toString('base64url'),
});
const token = auth.access_token;
const conversation = await request('/conversations', token, { request_id: randomUUID() });
const answer = `DOC-${randomBytes(5).toString('hex').toUpperCase()}`;
const body = `# Synthetic long document\n\n${'Intermediate line without the answer.\n'.repeat(750)}\nFinal answer: ${answer}\n`;
assert(body.length > 20_000, 'The answer must be beyond the first segment');
const document = await request(`/cases/${conversation.id}/documents`, token, {
  request_id: randomUUID(),
  filename: 'synthetic-long.md',
  content_type: 'text/markdown',
  data_base64: Buffer.from(body).toString('base64'),
});
const session = await request(`/cases/${conversation.id}/agent-sessions`, token, {});
const claim = await request(`/agent-sessions/${session.id}/claim`, token, {});
const probe = await fetch(`${origin}/internal/agent-sessions/${session.id}/attachments/read`, {
  method: 'POST',
  headers: { authorization: `Bearer ${claim.storage_token}`, 'content-type': 'application/json' },
  body: JSON.stringify({ document_id: document.id, offset: 20_000 }),
});
const probeBody = await probe.text();
assert(probe.ok, `Direct attachment read: ${probe.status} ${probeBody.slice(0, 1000)}`);
assert(probeBody.includes(answer), 'Direct attachment read did not reach the answer');
const sent = await request(`/cases/${conversation.id}/conversation/messages`, token, {
  client_message_id: randomUUID(),
  text: '请读取所附文档直到末尾，回答最后一行 Final answer 后的完整代号。不要猜测，也不要省略后半段。',
  attachment_ids: [document.id],
  settings: { model: 'deepseek-flash', effort: 'low', approval: 'ask' },
});

let completed;
let sessionId;
for (let attempt = 0; attempt < 360; attempt++) {
  const history = await request(`/cases/${conversation.id}/conversation`, token);
  sessionId = history.session_id;
  const message = history.items.find(item => item.id === sent.message.id);
  if (['completed', 'failed'].includes(message?.response?.state)) {
    completed = message.response;
    break;
  }
  await pause(500);
}
assert(completed, 'Document response timed out');
if (completed.state !== 'completed' || !completed.tools.some(tool => tool.name === 'attachment_read' && tool.state === 'completed')) {
  const claim = await request(`/agent-sessions/${sessionId}/claim`, token, {});
  const raw = await fetch(`${origin}/internal/agent-sessions/${sessionId}/attachments/read`, {
    method: 'POST',
    headers: { authorization: `Bearer ${claim.storage_token}`, 'content-type': 'application/json' },
    body: JSON.stringify({ document_id: document.id, offset: 20_000 }),
  });
  const body = await raw.text().catch(error => `response body failed: ${error}`);
  throw new Error(`Agent attachment read failed: tools=${JSON.stringify(completed.tools)}, direct=${raw.status} ${body.slice(0, 1000)}, reply=${completed.text.slice(0, 1000)}`);
}
assert(completed.text.includes(answer), `Agent did not recover the final answer: ${completed.text}`);
console.log(JSON.stringify({ passed: true, case_id: conversation.id, message_id: sent.message.id, document_id: document.id, characters: body.length, attachment_reads: completed.tools.filter(tool => tool.name === 'attachment_read').length }));
