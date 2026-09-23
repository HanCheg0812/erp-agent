// Explicit, synthetic acceptance against the configured model and RustFS.
import assert from 'node:assert/strict';
import { randomUUID, randomBytes } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { spawn } from 'node:child_process';
import { once } from 'node:events';

assert.equal(process.env.LIVE_ACCEPTANCE, '1', 'Set LIVE_ACCEPTANCE=1 to call the configured model');
const imagePath = process.env.LIVE_VISION_IMAGE;
assert(imagePath, 'Set LIVE_VISION_IMAGE to a synthetic PNG showing 7392');
const image = await readFile(imagePath);
const port = Number(process.env.LIVE_VISION_PORT ?? 3201);
const origin = `http://127.0.0.1:${port}`;
assert.equal(await fetch(`${origin}/health`).then(() => true, () => false), false, 'Port already occupied');
const binary = new URL('../services/api/target/debug/erp-agent-api', import.meta.url).pathname;
const child = spawn(binary, [], { env: { ...process.env, API_PORT: String(port) }, stdio: ['ignore', 'ignore', 'pipe'] });
let logs = '';
child.stderr.on('data', chunk => { logs += chunk.toString(); });
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));

async function request(path, token, body) {
  const response = await fetch(`${origin}/v1${path}`, {
    method: body === undefined ? 'GET' : 'POST',
    headers: { ...(token ? { authorization: `Bearer ${token}` } : {}), ...(body === undefined ? {} : { 'content-type': 'application/json' }) },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });
  const result = await response.json();
  assert(response.ok, `${path}: ${response.status} ${JSON.stringify(result)}`);
  return result;
}

try {
  let ready = false;
  for (let attempt = 0; attempt < 100; attempt++) {
    if (await fetch(`${origin}/ready`).then(response => response.ok, () => false)) { ready = true; break; }
    await pause(100);
  }
  assert(ready, `API did not start: ${logs}`);
  const suffix = randomBytes(6).toString('hex');
  const auth = await request('/auth/register', undefined, { email: `vision-${suffix}@example.test`, password: randomBytes(24).toString('base64url') });
  const token = auth.access_token;
  const conversation = await request('/conversations', token, { request_id: randomUUID() });
  const document = await request(`/cases/${conversation.id}/documents`, token, {
    request_id: randomUUID(), filename: 'synthetic-7392.png', content_type: 'image/png', data_base64: image.toString('base64'),
  });
  const sent = await request(`/cases/${conversation.id}/conversation/messages`, token, {
    client_message_id: randomUUID(), text: '请只回答这张图片中央的四位数字。', attachment_ids: [document.id],
    settings: { model: 'deepseek-flash', effort: 'low', approval: 'ask' },
  });
  assert.equal(sent.message.attachments[0].id, document.id);
  let complete;
  for (let attempt = 0; attempt < 240; attempt++) {
    const history = await request(`/cases/${conversation.id}/conversation`, token);
    const message = history.items.find(item => item.id === sent.message.id);
    assert.equal(message.attachments[0].id, document.id);
    if (['completed', 'failed'].includes(message.response?.state)) { complete = message.response; break; }
    await pause(500);
  }
  assert(complete, 'Vision response timed out');
  assert.equal(complete.state, 'completed', `Vision response failed: ${complete.error}`);
  assert.match(complete.text, /7392/, `Vision result did not read synthetic number: ${complete.text}`);
  console.log(JSON.stringify({ passed: true, case_id: conversation.id, message_id: sent.message.id, image_bytes: image.length, response: complete.text }));
} finally {
  if (child.exitCode === null) { child.kill('SIGTERM'); await once(child, 'exit'); }
}
