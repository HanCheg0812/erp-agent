import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import pg from 'pg';

const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright');
const origin = process.env.VERIFY_WEB_ORIGIN ?? 'http://127.0.0.1:4173';
const output = process.env.VERIFY_CONTROLS_OUTPUT ?? '/tmp/erp-mcp-ui';
const report = { checks: [], screenshots: [], page_errors: [] };
await mkdir(output, { recursive: true });
const browser = await chromium.launch({ executablePath: '/usr/bin/google-chrome', headless: true });
const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
page.on('pageerror', error => report.page_errors.push(error.message));
page.setDefaultTimeout(15_000);
let token;
async function api(path) {
  const r = await page.request.get(`${origin}/v1${path}`, { headers: { authorization: `Bearer ${token}` } });
  assert.equal(r.status(), 200); return r.json();
}
async function capture(name) {
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false, `${name}: page overflow`);
  const composer = await page.locator('.chat-composer').boundingBox();
  const send = await page.getByRole('button', { name: 'Send message', exact: true }).boundingBox();
  const mic = await page.getByRole('button', { name: 'Voice input', exact: true }).boundingBox();
  const add = await page.getByRole('button', { name: 'Add attachment', exact: true }).boundingBox();
  assert(composer && composer.y >= 0 && composer.y + composer.height <= page.viewportSize().height + 1);
  assert(composer.x >= 0 && composer.x + composer.width <= page.viewportSize().width + 1, `${name}: composer must fit viewport`);
  const header = await page.locator('.chat-header').boundingBox();
  assert(header && header.x >= 0 && header.x + header.width <= page.viewportSize().width + 1, `${name}: header must fit viewport`);
  for (const box of [send, mic]) assert(box && box.x >= composer.x && box.x + box.width <= composer.x + composer.width);
  assert(Math.abs(send.y + send.height / 2 - add.y - add.height / 2) < 3, `${name}: send must remain on the toolbar row`);
  await page.screenshot({ path: `${output}/${name}.png` }); report.screenshots.push(name);
}
async function waitReply(id, until) {
  for (let i = 0; i < 165; i++) {
    const history = await api(`/cases/${id}/conversation`);
    if (until(history.items.at(-1)?.response)) return history;
    await new Promise(resolve => setTimeout(resolve, 600));
  }
  throw Error('Agent response deadline');
}
async function send(text) {
  await page.getByRole('textbox', { name: 'Message agent' }).fill(text);
  const request = page.waitForResponse(r => r.url().endsWith('/conversation/messages') && r.request().method() === 'POST');
  await page.getByRole('button', { name: 'Send message', exact: true }).click();
  const r = await request; assert.equal(r.status(), 201); return r.json();
}
try {
  await page.goto(origin);
  await page.getByLabel('Email', { exact: true }).fill(process.env.DEMO_EMAIL);
  await page.getByLabel('Password', { exact: true }).fill(process.env.DEMO_PASSWORD);
  await page.getByRole('button', { name: 'Sign in', exact: true }).click();
  await page.locator('.chat-app').waitFor();
  token = await page.evaluate(() => localStorage.getItem('erp-agent-token'));
  await page.locator('.conversation-item').filter({ hasText: '请先用 ERP 工具列出当前有权限' }).first().click();
  await page.locator('.answer-text .agent-markdown').filter({ hasText: 'SAL-ORD-' }).waitFor();
  await page.locator('.agent-table-scroll table').waitFor();
  await capture('desktop-chat');
  const thinkingToggle = page.locator('.thinking-toggle');
  if (await thinkingToggle.getAttribute('aria-expanded') === 'false') await thinkingToggle.click();
  for (const label of [/^查看订单已完成$/, /^查看订单详情已完成$/, /^搜索业务资料失败$/]) await page.locator('.process-tool-name').filter({ hasText: label }).waitFor();
  await page.locator('.process-tool-name').filter({ hasText: /^查看订单详情已完成$/ }).scrollIntoViewIfNeeded();
  await capture('desktop-tools');
  await thinkingToggle.click();
  report.checks.push('Real ERP list/read and failed unconfigured OpenViking call render in the expandable timeline');
  for (const width of [1440, 390, 320]) {
    await page.setViewportSize({ width, height: width === 1440 ? 1000 : 844 });
    await capture(`composer-${width}`);
    await page.getByRole('button', { name: 'Model and reasoning' }).click();
    await page.locator('.endpoint-details summary').click();
    await page.getByText('https://api.deepseek.com', { exact: true }).waitFor();
    await capture(`reasoning-${width}`);
    await page.getByRole('button', { name: 'Choose model', exact: true }).click();
    await capture(`model-${width}`);
    await page.keyboard.press('Escape');
    await page.getByRole('button', { name: 'Agent approval mode', exact: true }).click();
    await capture(`access-${width}`);
    await page.keyboard.press('Escape');
    const input=page.getByRole('textbox',{name:'Message agent'});
    await input.fill('短句');const initial=await page.locator('.chat-composer').boundingBox();
    await input.fill(Array(6).fill('输入框随着换行向上长高。').join('\n'));const grown=await page.locator('.chat-composer').boundingBox();
    assert(grown.height>initial.height&&grown.y<initial.y);assert(Math.abs(grown.y+grown.height-initial.y-initial.height)<2);
    await capture(`composer-grown-${width}`);
    await input.fill(Array(30).fill('超出高度后，输入框内部滚动。').join('\n'));
    assert(await input.evaluate(n=>n.scrollHeight>n.clientHeight&&getComputedStyle(n).overflowY==='auto'));
    await capture(`composer-capped-${width}`);
    await input.fill('');assert(Math.abs((await page.locator('.chat-composer').boundingBox()).height-initial.height)<2);
  }
  report.checks.push('Composer, mic/send controls, access and real endpoint/model/effort menus fit 1440/390/320 widths');
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.getByRole('button', { name: 'New conversation', exact: true }).click();
  await page.getByRole('heading', { name: 'New conversation', exact: true }).waitFor();
  assert.equal(await page.getByRole('dialog').count(), 0);
  const record = (await api('/cases')).items[0]; report.case_id = record.id;
  await page.getByRole('button', { name: 'Agent approval mode' }).click();
  await page.getByRole('group', { name: 'Approval modes' }).getByRole('button', { name: '完全访问', exact: false }).click();
  await page.getByRole('button', { name: 'Model and reasoning' }).click();
  await page.getByRole('button', { name: 'Choose model', exact: true }).click();
  await page.getByRole('button', { name: 'DeepSeek Pro', exact: true }).click();
  await page.getByRole('button', { name: 'Disable reasoning', exact: true }).click();
  await page.keyboard.press('Escape');
  await send('只回复“设置已生效”，不要调用任何工具。');
  let history = await waitReply(record.id, r => r && ['completed','failed'].includes(r.state));
  assert.equal(history.items.at(-1).response.state, 'completed');
  assert.deepEqual(history.settings, { approval: 'full', model: 'deepseek-v4-pro', effort: 'none' });
  assert.deepEqual(history.items.at(-1).response.tools, []);
  await page.reload();
  await page.getByRole('button', { name: 'Model and reasoning' }).filter({ hasText: 'DeepSeek Pro' }).waitFor();
  await page.getByRole('button', { name: 'Agent approval mode' }).filter({ hasText: '完全访问' }).waitFor();
  await page.getByRole('button', { name: 'Model and reasoning' }).filter({ hasText: '关闭' }).waitFor();
  report.checks.push('Pro + reasoning off + full approval mode completes via real Responses API; settings survive refresh');
  await page.getByRole('button', { name: 'Model and reasoning' }).click();
  await page.getByRole('button', { name: 'Choose model', exact: true }).click();
  await page.getByRole('button', { name: 'DeepSeek Flash', exact: true }).click();
  await page.getByRole('slider', { name: '思考程度', exact: true }).fill('3');
  await page.keyboard.press('Escape');
  await send('请比较制造业订单交期优化的二十种策略，每种说明适用条件和局限，详细分析。');
  history = await waitReply(record.id, r => r?.state === 'running' && r.steps?.some(s => s.kind === 'reasoning'));
  await page.getByRole('button', { name: 'Stop response', exact: true }).click();
  history = await waitReply(record.id, r => r?.state === 'cancelled');
  assert.equal(history.items.at(-1).response.state, 'cancelled');
  const db = new pg.Client({ connectionString: process.env.DATABASE_URL }); await db.connect();
  try {
    const metadata = (await db.query('SELECT metadata FROM agent_sessions WHERE id=$1', [history.session_id])).rows[0].metadata;
    assert.equal(metadata.model, 'deepseek-flash'); assert.equal(metadata.reasoning_effort, 'max');
    report.persisted_model = metadata.model; report.persisted_effort = metadata.reasoning_effort;
  } finally { await db.end(); }
  await page.getByText('Stopped', { exact: true }).waitFor();
  await page.getByRole('button', { name: 'Send message', exact: true }).waitFor();
  await capture('stopped-desktop');
  await page.reload();
  await page.getByText('Stopped', { exact: true }).waitFor();
  report.checks.push('Changing model/effort on a resumed session reaches JCode; Stop cancels the real running child and persists after reload');
  assert.deepEqual(report.page_errors, []); report.passed = true;
} catch (error) {
  report.passed = false; report.failure = error.message;
  await page.screenshot({ path: `${output}/failure.png` }).catch(() => {}); throw error;
} finally {
  await writeFile(`${output}/report.json`, JSON.stringify(report, null, 2)); await browser.close();
}
console.log(`PASS ${report.checks.length} real Agent UI checks; screenshots: ${output}`);
