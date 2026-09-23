import assert from 'node:assert/strict';
import { randomUUID, randomBytes } from 'node:crypto';
import { mkdir, writeFile } from 'node:fs/promises';
import pg from 'pg';

const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright');
const origin = process.env.VERIFY_WEB_ORIGIN ?? 'http://127.0.0.1:4173';
const output = process.env.VERIFY_SETTINGS_OUTPUT ?? '/tmp/erp-settings-sync';
await mkdir(output, { recursive: true });
const report = { checks: [], replies: [], page_errors: [] };
const labels = { ask: '\u8bf7\u6c42\u6279\u51c6', auto: '\u5e2e\u6211\u6279\u51c6', full: '\u5b8c\u5168\u8bbf\u95ee' };
let token;
let caseId;
async function api(path, body, method = body ? 'POST' : 'GET') {
  const response = await fetch(`${origin}/v1${path}`, {
    method, headers: { 'content-type': 'application/json', ...(token ? { authorization: `Bearer ${token}` } : {}) },
    ...(body ? { body: JSON.stringify(body) } : {}),
  });
  const value = response.status === 204 ? null : await response.json();
  assert(response.ok, `${path}: ${response.status} ${JSON.stringify(value)}`);
  return value;
}
const db = new pg.Client({ connectionString: process.env.DATABASE_URL });
await db.connect();
const browser = await chromium.launch({ executablePath: '/usr/bin/google-chrome', headless: true });
const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
page.on('pageerror', error => report.page_errors.push(error.message));
const settingsRequest = request => request.method() === 'PATCH' && request.url().endsWith('/conversation/settings');
const settingsResponse = response => settingsRequest(response.request());
async function choose(mode) {
  await page.getByRole('button', { name: 'Agent approval mode' }).click();
  await page.getByRole('group', { name: 'Approval modes' }).getByRole('button', { name: new RegExp(labels[mode]) }).click();
}
async function savedMode(mode) {
  const response = page.waitForResponse(settingsResponse);
  await choose(mode);
  assert.equal((await response).status(), 200);
  await page.locator('.chat-composer[aria-busy="false"]').waitFor();
  assert.equal((await api(`/cases/${caseId}/conversation`)).settings.approval, mode);
  assert.equal(await page.getByRole('button', { name: 'Agent approval mode' }).textContent(), labels[mode]);
}
async function completed(messageId) {
  for (let attempt = 0; attempt < 240; attempt++) {
    const history = await api(`/cases/${caseId}/conversation`);
    const response = history.items.find(item => item.id === messageId)?.response;
    assert.notEqual(response?.state, 'failed', JSON.stringify(response));
    if (response?.state === 'completed') return history;
    await new Promise(resolve => setTimeout(resolve, 600));
  }
  throw Error('Verification wait expired');
}
async function verifyReply(message, mode) {
  const history = await completed(message.id);
  const response = history.items.find(item => item.id === message.id).response;
  assert.equal(response.text.trim().replace(/[.`"\s]/g, ''), mode, response.text);
  const runs = (await db.query('SELECT settings FROM agent_runs WHERE message_id=$1', [message.id])).rows;
  assert.equal(runs.length, 1);
  assert.equal(runs[0].settings.approval, mode);
  assert.equal(history.settings.approval, mode);
  report.replies.push({ mode, text: response.text, message_id: message.id, session_id: history.session_id });
  await page.getByRole('button', { name: 'Stop response' }).waitFor({ state: 'hidden' });
  report.checks.push(`Same session: UI, PostgreSQL, run settings and real model agree on ${mode}`);
}
const prompt = 'Report only the current backend-injected permission mode, using exactly one of ask, auto, full. Do not call tools. Do not infer the mode from prior conversation.';
try {
  token = (await api('/auth/register', { email: `settings-${randomUUID()}@example.test`, password: randomBytes(24).toString('hex') })).access_token;
  await page.goto(origin);
  await page.evaluate(value => localStorage.setItem('erp-agent-token', value), token);
  caseId = (await api('/conversations', { request_id: randomUUID() })).id;
  report.case_id = caseId;
  await page.reload();
  await page.getByRole('button', { name: 'Agent approval mode' }).waitFor();

  await savedMode('auto');
  assert.equal((await api(`/cases/${caseId}/conversation`)).items.length, 0);
  await page.reload();
  await page.getByRole('button', { name: 'Agent approval mode' }).waitFor();
  assert.equal(await page.getByRole('button', { name: 'Agent approval mode' }).textContent(), labels.auto);
  report.checks.push('Changing mode persists before any message and survives reload');

  // Hold the first write so later clicks must queue behind it.
  let release;
  const held = new Promise(resolve => { release = resolve; });
  const writes = [];
  await page.route('**/conversation/settings', async route => {
    writes.push(route.request().postDataJSON().approval);
    if (writes.length === 1) await held;
    await route.continue();
  });
  const firstWrite = page.waitForRequest(settingsRequest);
  await choose('full');
  await firstWrite;
  await choose('ask');
  assert.deepEqual(writes, ['full']);
  release();
  await page.locator('.chat-composer[aria-busy="false"]').waitFor();
  assert.deepEqual(writes, ['full', 'ask']);
  assert.equal((await api(`/cases/${caseId}/conversation`)).settings.approval, 'ask');
  await page.unroute('**/conversation/settings');
  report.checks.push('Rapid changes are saved in selection order');

  // Two failed optimistic changes must restore the last confirmed mode.
  let failRelease;
  const failedHeld = new Promise(resolve => { failRelease = resolve; });
  let failures = 0;
  await page.route('**/conversation/settings', async route => {
    if (++failures === 1) await failedHeld;
    await route.fulfill({ status: 503, contentType: 'application/json', body: JSON.stringify({ message: 'settings_test_unavailable' }) });
  });
  const failedWrite = page.waitForRequest(settingsRequest);
  await choose('full');
  await failedWrite;
  await choose('auto');
  failRelease();
  await page.locator('.chat-composer[aria-busy="false"]').waitFor();
  assert.equal(failures, 2);
  assert.equal(await page.getByRole('button', { name: 'Agent approval mode' }).textContent(), labels.ask);
  assert.equal((await api(`/cases/${caseId}/conversation`)).settings.approval, 'ask');
  await page.getByRole('alert').filter({ hasText: 'settings_test_unavailable' }).waitFor();
  await page.unroute('**/conversation/settings');
  report.checks.push('Failed saves show an error and restore the last backend-confirmed mode');

  const effortResponse = page.waitForResponse(settingsResponse);
  await page.getByRole('button', { name: 'Model and reasoning' }).click();
  await page.getByRole('button', { name: 'Disable reasoning' }).click();
  assert.equal((await effortResponse).status(), 200);
  await page.keyboard.press('Escape');
  await page.locator('.chat-composer[aria-busy="false"]').waitFor();
  for (const mode of ['ask', 'auto', 'full', 'ask']) {
    await savedMode(mode);
    await page.getByRole('textbox', { name: 'Message agent' }).fill(prompt);
    const sent = page.waitForResponse(response => response.request().method() === 'POST' && response.url().endsWith('/conversation/messages'));
    await page.getByRole('button', { name: 'Send message' }).click();
    const response = await sent;
    assert.equal(response.status(), 201);
    await verifyReply((await response.json()).message, mode);
  }
  assert.equal(new Set(report.replies.map(reply => reply.session_id)).size, 1);

  let sendRelease;
  const sendHeld = new Promise(resolve => { sendRelease = resolve; });
  await page.route('**/conversation/settings', async route => { await sendHeld; await route.continue(); });
  const saving = page.waitForRequest(settingsRequest);
  await choose('auto');
  await saving;
  await page.getByRole('textbox', { name: 'Message agent' }).fill(prompt);
  let posted = false;
  const observeSend = request => { if (request.method() === 'POST' && request.url().endsWith('/conversation/messages')) posted = true; };
  page.on('request', observeSend);
  const sent = page.waitForResponse(response => response.request().method() === 'POST' && response.url().endsWith('/conversation/messages'));
  await page.getByRole('button', { name: 'Send message' }).click();
  await page.getByRole('textbox', { name: 'Message agent' }).isDisabled().then(value => assert.equal(value, true));
  assert.equal(posted, false);
  sendRelease();
  const response = await sent;
  assert.equal(response.request().postDataJSON().settings.approval, 'auto');
  await verifyReply((await response.json()).message, 'auto');
  page.off('request', observeSend);
  await page.unroute('**/conversation/settings');
  report.checks.push('Sending while a change is pending waits for persistence and uses the selected mode');

  for (const width of [1440, 390]) {
    await page.setViewportSize({ width, height: width === 1440 ? 1000 : 844 });
    await page.reload();
    await page.getByRole('button', { name: 'Agent approval mode' }).waitFor();
    await page.waitForFunction(() => !document.querySelector('[aria-label="Agent approval mode"]')?.disabled);
    assert.equal(await page.getByRole('button', { name: 'Agent approval mode' }).textContent(), labels.auto);
    assert.match(await page.getByRole('button', { name: 'Model and reasoning' }).textContent(), /\u5173\u95ed/);
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
    await page.screenshot({ path: `${output}/settings-${width}.png` });
  }
  assert.deepEqual(report.page_errors, []);
  report.passed = true;
} catch (error) {
  report.passed = false;
  report.failure = error.message;
  await page.screenshot({ path: `${output}/failure.png` }).catch(() => {});
  throw error;
} finally {
  if (caseId) {
    const history = await api(`/cases/${caseId}/conversation`);
    for (const item of history.items) if (['queued', 'running', 'waiting'].includes(item.response?.state)) {
      await api(`/cases/${caseId}/conversation/cancel`, { message_id: item.id });
    }
  }
  await writeFile(`${output}/report.json`, JSON.stringify(report, null, 2));
  await browser.close();
  await db.end();
}
console.log(`PASS ${report.checks.length} settings synchronization checks; screenshots: ${output}`);
