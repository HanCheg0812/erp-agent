import assert from 'node:assert/strict';
import { randomUUID, randomBytes } from 'node:crypto';
import { mkdir, writeFile } from 'node:fs/promises';
import pg from 'pg';

const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright');
const origin = process.env.VERIFY_WEB_ORIGIN ?? 'http://127.0.0.1:4173';
const output = process.env.VERIFY_APPROVAL_OUTPUT ?? '/tmp/erp-tool-approvals';
await mkdir(output, { recursive: true });
const report = { checks: [], page_errors: [] };
const createdCases = [];
let token;
async function api(path, body, method = body ? 'POST' : 'GET') {
  const r = await fetch(`${origin}/v1${path}`, { method, headers: { 'content-type': 'application/json', ...(token ? { authorization: `Bearer ${token}` } : {}) }, ...(body ? { body: JSON.stringify(body) } : {}) });
  const value = r.status === 204 ? null : await r.json();
  assert(r.ok, `${path}: ${r.status} ${JSON.stringify(value)}`); return value;
}
async function waitFor(caseId, predicate) {
  for (let i = 0; i < 240; i++) {
    const h = await api(`/cases/${caseId}/conversation`);
    const response = h.items.at(-1)?.response;
    assert.notEqual(response?.state, 'failed', JSON.stringify(response));
    if (predicate(response)) return h;
    await new Promise(resolve => setTimeout(resolve, 600));
  }
  throw Error('Approval deadline');
}
const db = new pg.Client({ connectionString: process.env.DATABASE_URL });
await db.connect();
const browser = await chromium.launch({ executablePath: '/usr/bin/google-chrome', headless: true });
const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
page.on('pageerror', error => report.page_errors.push(error.message));
try {
  const email = `approval-${randomUUID()}@example.test`;
  token = (await api('/auth/register', { email, password: randomBytes(24).toString('hex') })).access_token;
  report.user = (await api('/auth/me')).userId;
  await page.goto(origin);
  await page.evaluate(t => localStorage.setItem('erp-agent-token', t), token);
  for (const [mode, decision, text] of [
    ['ask', 'allowed', '我的报表偏好是数量使用件作为单位。'],
    ['ask', 'denied', '我的日期格式偏好是年月日。'],
    ['auto', 'allowed', '我的回复语言偏好是简体中文。'],
    ['full', 'allowed', '我的工作习惯是先看摘要再看明细。'],
  ]) {
    await page.setViewportSize({ width: 1440, height: 1000 });
    const record = await api('/conversations', { request_id: randomUUID() });
    createdCases.push(record.id);
    const sent = await api(`/cases/${record.id}/conversation/messages`, { client_message_id: randomUUID(), text: `请调用 memory_save 保存这条个人偏好：${text} 保存后简短回复。`, settings: { approval: mode, model: 'deepseek-flash', effort: 'low' } });
    let h = await waitFor(record.id, r => mode === 'ask' ? r?.state === 'waiting' : r?.state === 'completed');
    if (mode === 'ask') {
      const p = h.items.at(-1).response.permissions[0];
      assert.equal(p.state, 'pending');
      assert.equal((await api('/memory')).items.some(m => m.id === p.id), false);
      await page.reload();
      await page.locator('.conversation-item').first().click();
      await page.getByRole('button', { name: '批准', exact: true }).waitFor();
      for (const width of [1440, 390, 320]) {
        await page.setViewportSize({ width, height: width === 1440 ? 1000 : 844 });
        await page.locator('.tool-permission').scrollIntoViewIfNeeded();
        assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
        await page.screenshot({ path: `${output}/pending-${decision}-${width}.png` });
      }
      await page.reload();
      await page.getByRole('button', { name: decision === 'allowed' ? '批准' : '拒绝', exact: true }).click();
      h = await waitFor(record.id, r => r?.state === 'completed');
      report.checks.push(`${mode}/${decision}: durable card survives reload and original turn resumes`);
    }
    const response = h.items.at(-1).response;
    assert.equal(response.permissions.length, 1, JSON.stringify(response));
    const p = response.permissions[0];
    assert.equal(p.state, decision === 'allowed' ? 'executed' : 'denied', JSON.stringify(p));
    assert.equal(p.decided_by, mode === 'auto' ? 'reviewer' : mode === 'full' ? 'full' : 'user');
    const entries = (await db.query("SELECT e.payload FROM agent_session_entries e JOIN agent_session_heads h USING(session_id,kind,generation) WHERE e.session_id=$1 AND e.kind='messages' ORDER BY e.position", [h.session_id])).rows.map(r => r.payload);
    const blocks = entries.flatMap(m => m.content);
    assert.equal(blocks.filter(b => b.type === 'tool_use' && b.id === p.tool_call_id).length, 1);
    assert.equal(blocks.filter(b => b.type === 'tool_result' && b.tool_use_id === p.tool_call_id).length, 1);
    assert.equal(Number((await db.query('SELECT count(*) FROM case_conversation_messages WHERE case_id=$1', [record.id])).rows[0].count), 1);
    assert.equal((await api('/memory')).items.filter(m => m.id === p.id).length, decision === 'allowed' ? 1 : 0);
    report.checks.push(`${mode}/${decision}: one original tool call/result, one user turn, correct durable memory effect`);
    report[`${mode}-${decision}`] = { case_id: record.id, message_id: sent.message.id, permission_id: p.id, state: p.state, reply: response.text };
    await page.setViewportSize({ width: 1440, height: 1000 });
    await page.reload(); await page.locator('.conversation-item').first().click();
    await page.locator('.tool-permission').waitFor();
    await page.screenshot({ path: `${output}/finished-${mode}-${decision}.png` });
  }
  assert.deepEqual(report.page_errors, []); report.passed = true;
} catch (error) {
  report.passed = false; report.failure = error.message;
  await page.screenshot({ path: `${output}/failure.png` }).catch(() => {}); throw error;
} finally {
  if (token) for (const id of createdCases) {
    const h = await api(`/cases/${id}/conversation`);
    for (const item of h.items) if (['queued', 'running', 'waiting'].includes(item.response?.state)) {
      await api(`/cases/${id}/conversation/cancel`, { message_id: item.id });
    }
  }
  if (token) for (const m of (await api('/memory')).items) await api(`/memory/${m.id}`, undefined, 'DELETE');
  await writeFile(`${output}/report.json`, JSON.stringify(report, null, 2));
  await browser.close(); await db.end();
}
console.log(`PASS ${report.checks.length} real JCode approval checks; screenshots: ${output}`);
