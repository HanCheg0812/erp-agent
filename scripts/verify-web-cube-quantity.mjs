import assert from 'node:assert/strict';
import { mkdir, readFile, writeFile } from 'node:fs/promises';

const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright');
const origin = process.env.VERIFY_WEB_ORIGIN ?? 'http://127.0.0.1:4173';
const output = process.env.VERIFY_QUANTITY_UI_OUTPUT ?? `/tmp/erp-cube-quantity-ui/${Date.now()}`;
assert(process.env.VERIFY_QUANTITY_REPORT, 'A completed live acceptance report is required');
const live = JSON.parse(await readFile(process.env.VERIFY_QUANTITY_REPORT, 'utf8'));
assert.equal(live.passed, true);
const last = live.rounds.at(-1);
const report = { case_id: live.case_id, message_id: last.message_id, checks: [], screenshots: [],
  page_errors: [], http_errors: [] };
await mkdir(output, { recursive: true });
const browser = await chromium.launch({ executablePath: '/usr/bin/google-chrome', headless: true });
const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
page.setDefaultTimeout(15_000);
page.on('pageerror', error => report.page_errors.push(error.message));
page.on('response', response => {
  if (response.url().startsWith(origin) && response.status() >= 400) {
    report.http_errors.push({ path: new URL(response.url()).pathname, status: response.status() });
  }
});

async function inspect(width, suffix = '') {
  await page.setViewportSize({ width, height: width === 1440 ? 1000 : 844 });
  const reply = page.locator('.assistant-message').filter({ hasText: last.turn.text.split('\n')[0] }).last();
  await reply.waitFor();
  const toggle = reply.locator('.thinking-toggle');
  if (await toggle.getAttribute('aria-expanded') === 'false') await toggle.click();
  const tool = reply.locator('.process-tool-name');
  await tool.filter({ hasText: /^汇总数量已完成$/ }).waitFor();
  assert.equal(await tool.count(), 1);
  await reply.scrollIntoViewIfNeeded();
  const text = await reply.innerText();
  assert(text.includes(`TOTAL=${last.expected.total}`));
  assert(!text.includes('cube_quantity_summary'), 'The UI must use a business label');
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false,
    `Horizontal overflow at ${width}px`);
  const viewport = page.viewportSize();
  const composer = await page.locator('.chat-composer').boundingBox();
  assert(composer && composer.x >= 0 && composer.y >= 0
    && composer.x + composer.width <= viewport.width + 1
    && composer.y + composer.height <= viewport.height + 1, 'Composer must fit the viewport');
  for (const name of ['Add attachment', 'Send message', 'Model and reasoning', 'Agent approval mode']) {
    const box = await page.getByRole('button', { name, exact: true }).boundingBox();
    assert(box && box.x >= 0 && box.x + box.width <= viewport.width + 1
      && box.y >= 0 && box.y + box.height <= viewport.height + 1, `${name} must fit the viewport`);
  }
  const path = `${output}/quantity-${width}${suffix}.png`;
  await page.screenshot({ path });
  report.screenshots.push(path);
  report.checks.push({ width, suffix, tool_text: await tool.innerText(), reply_text: text });
}

try {
  await page.goto(origin);
  await page.getByLabel('Email', { exact: true }).fill(process.env.DEMO_EMAIL);
  await page.getByLabel('Password', { exact: true }).fill(process.env.DEMO_PASSWORD);
  await page.getByRole('button', { name: 'Sign in', exact: true }).click();
  await page.locator('.chat-app').waitFor();
  const token = await page.evaluate(() => localStorage.getItem('erp-agent-token'));
  const detail = await page.request.get(`${origin}/v1/cases/${live.case_id}`, {
    headers: { authorization: `Bearer ${token}` },
  });
  assert.equal(detail.status(), 200);
  const record = await detail.json();
  const select = async () => {
    await page.locator('.conversation-item').filter({
      has: page.locator('.conversation-title').getByText(record.title, { exact: true }),
    }).first().click();
    await page.locator('.answer-text').filter({ hasText: last.turn.text.split('\n')[0] }).last().waitFor();
  };
  await select();
  for (const width of [1440, 390, 320]) await inspect(width);
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.reload();
  await page.locator('.chat-app').waitFor();
  await select();
  await inspect(1440, '-reload');
  assert.deepEqual(report.page_errors, []);
  assert.deepEqual(report.http_errors, []);
  report.passed = true;
} catch (error) {
  report.passed = false;
  report.failure = error.message;
  process.exitCode = 1;
  await page.screenshot({ path: `${output}/failure.png` }).catch(() => {});
} finally {
  await writeFile(`${output}/report.json`, JSON.stringify(report, null, 2));
  await browser.close();
}
console.log(JSON.stringify({ passed: report.passed, failure: report.failure,
  screenshots: report.screenshots, report: `${output}/report.json` }, null, 2));
