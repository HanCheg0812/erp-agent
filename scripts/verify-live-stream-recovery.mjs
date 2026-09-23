import assert from 'node:assert/strict';
import { randomBytes, randomUUID } from 'node:crypto';
import { mkdir, writeFile } from 'node:fs/promises';
import pg from 'pg';

const origin = process.env.VERIFY_WEB_ORIGIN ?? 'http://127.0.0.1:4173';
const output = '/tmp/erp-live-stream-recovery';
await mkdir(output, { recursive: true });
const report = { checks: [], page_errors: [] };
const db = new pg.Client({ connectionString: process.env.DATABASE_URL }); await db.connect();
let token; let caseId; let browser;
async function api(path, body) {
  const r = await fetch(`${origin}/v1${path}`, { method: body ? 'POST' : 'GET', headers: { 'content-type': 'application/json', ...(token ? { authorization: `Bearer ${token}` } : {}) }, ...(body ? { body: JSON.stringify(body) } : {}) });
  const value = await r.json(); assert(r.ok, `${path}: ${r.status}`); return value;
}
async function waitFor(predicate) {
  for (let i = 0; i < 240; i++) {
    const history = await api(`/cases/${caseId}/conversation`);
    const response = history.items.at(-1)?.response;
    assert.notEqual(response?.state, 'failed', JSON.stringify(response));
    if (predicate(response)) return history;
    await new Promise(resolve => setTimeout(resolve, 500));
  }
  throw Error('Live verification did not finish');
}
try {
  token = (await api('/auth/register', { email: `stream-${randomUUID()}@example.test`, password: randomBytes(24).toString('hex') })).access_token;
  caseId = (await api('/conversations', { request_id: randomUUID() })).id;
  report.case_id = caseId;
  const sent = await api(`/cases/${caseId}/conversation/messages`, { client_message_id: randomUUID(), text: '这是合成验收，不调用工具。第一行写 STREAM-CHECK-7391。然后列出制造业交付管理的 40 个不同改进点，每点用一段解释其适用范围和局限。', settings: { model: 'deepseek-v4-pro', effort: 'high', approval: 'ask' } });
  await waitFor(r => r?.text?.length > 250);
  await api(`/cases/${caseId}/conversation/cancel`, { message_id: sent.message.id });
  let history = await waitFor(r => r?.state === 'cancelled');
  const partial = history.items.at(-1).response.text;
  assert(partial.includes('STREAM-CHECK-7391')); report.saved_characters = partial.length;
  const messages = (await db.query("SELECT e.payload FROM agent_session_entries e JOIN agent_session_heads h USING(session_id,kind,generation) WHERE e.session_id=$1 AND e.kind='messages' AND e.position<h.length ORDER BY e.position", [history.session_id])).rows.map(r => r.payload);
  assert(messages.some(m => m.role === 'assistant' && m.content.some(b => b.type === 'text' && b.text === partial)));
  report.checks.push('Real DeepSeek Pro cancellation retains exactly the displayed answer in native PostgreSQL history');
  await api(`/cases/${caseId}/conversation/messages`, { client_message_id: randomUUID(), text: '上一条回复被我主动停止。只回复你上一条已输出正文的第一行标记，不要使用工具。', settings: { model: 'deepseek-flash', effort: 'none', approval: 'ask' } });
  history = await waitFor(r => r?.state === 'completed');
  assert(history.items.at(-1).response.text.includes('STREAM-CHECK-7391'));
  report.checks.push('Next real model turn recalls the saved assistant answer after the runtime process was replaced');
  const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright');
  browser = await chromium.launch({ executablePath: '/usr/bin/google-chrome', headless: true });
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  page.on('pageerror', e => report.page_errors.push(e.message));
  await page.goto(origin); await page.evaluate(t => localStorage.setItem('erp-agent-token', t), token); await page.reload();
  await page.locator('.conversation-item').first().click();
  await page.getByText('STREAM-CHECK-7391', { exact: true }).last().waitFor();
  for (const width of [1440, 390]) {
    await page.setViewportSize({ width, height: width === 1440 ? 1000 : 844 });
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
    await page.screenshot({ path: `${output}/recovered-${width}.png` });
  }
  assert.deepEqual(report.page_errors, []); report.passed = true;
} catch (error) { report.passed = false; report.failure = error.message; throw error; }
finally {
  if (token && caseId) {
    const history = await api(`/cases/${caseId}/conversation`);
    for (const item of history.items) if (['queued', 'running', 'waiting'].includes(item.response?.state)) await api(`/cases/${caseId}/conversation/cancel`, { message_id: item.id });
  }
  await writeFile(`${output}/report.json`, JSON.stringify(report, null, 2));
  await browser?.close(); await db.end();
}
console.log(`PASS live cancellation, model recall and browser refresh; evidence: ${output}`);
