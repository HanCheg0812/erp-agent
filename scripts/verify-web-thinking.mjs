import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';

const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright');
const origin = process.env.VERIFY_WEB_ORIGIN ?? 'http://127.0.0.1:4173';
const output = process.env.VERIFY_THINKING_OUTPUT ?? '/tmp/erp-thinking-check';
await mkdir(output, { recursive: true });
const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH ?? '/usr/bin/google-chrome', headless: true });
const report = { checks: [], page_errors: [] };
const normalizeText = text => text.replace(/\s+/g, '');
try {
  const page = await browser.newPage();
  page.on('pageerror', error => report.page_errors.push(error.message));
  await page.route('**/v1/**', route => route.abort());
  for (const [width, height] of [[1440, 1000], [390, 844], [320, 844], [390, 480]]) {
    await page.setViewportSize({ width, height });
    await page.goto(origin);
    await page.getByRole('button', { name: 'View demo', exact: true }).click();
    await page.locator('.message-list').waitFor();
    // Mount the real reply inside the demo's real scrolling layout, without API writes.
    await page.evaluate(async () => {
      const main = await (await fetch('/src/main.tsx')).text();
      const moduleUrl = name => main.split('\n').find(line => line.includes(`/deps/${name}.js?`)).match(/from "([^"]+)"/)[1];
      const { default: React } = await import(moduleUrl('react'));
      const { default: ReactDOM } = await import(moduleUrl('react-dom_client'));
      const { AgentReply } = await import('/src/AgentReply.tsx');
      const list = document.querySelector('.message-list');
      for (const child of list.children) child.style.display = 'none';
      const host = document.createElement('div');
      host.id = 'thinking-probe';
      list.append(host);
      const root = ReactDOM.createRoot(host);
      window.probeText = '核对订单资料、交付日期和物料版本，整理需要确认的信息。\n\n'.repeat(50);
      window.renderThinking = (state = 'running', suffix = '') => {
        window.probeText += suffix;
        root.render(React.createElement(AgentReply, { response: {
          state, text: '', tools: [{ id: 'read', name: 'case_read', state: 'completed' }],
          steps: [
            { kind: 'reasoning', id: 'first', state: 'completed', text: '先读取当前案件，确认现有订单的版本与交付要求。\n\n'.repeat(25) },
            { kind: 'tool', id: 'read' },
            { kind: 'reasoning', id: 'live', state, text: window.probeText },
          ],
        } }));
      };
      window.renderThinking();
    });
    const toggle = page.locator('#thinking-probe .thinking-toggle');
    const viewport = page.locator('#thinking-probe .reasoning-text').last();
    const firstViewport = page.locator('#thinking-probe .reasoning-text').first();
    const lastText = page.locator('#thinking-probe .reasoning-text').last();
    await toggle.waitFor();
    assert.equal(await page.locator('.thinking-collapse').evaluate(el => el.getBoundingClientRect().height), 0);
    await toggle.click();
    const fullText = await page.evaluate(() => window.probeText);
    assert.equal(normalizeText(await lastText.textContent()), normalizeText(fullText), 'First open shows all received text');
    const dimensions = await viewport.evaluate(el => ({ height: el.clientHeight, scrollHeight: el.scrollHeight, cap: Math.min(320, innerHeight * 0.45), overflow: el.scrollWidth > el.clientWidth }));
    assert(dimensions.height <= dimensions.cap && dimensions.scrollHeight > dimensions.height);
    assert.equal(dimensions.overflow, false);
    assert(await page.locator('#thinking-probe .thinking-content').evaluate(el => {
      const style = getComputedStyle(el);
      return style.borderTopWidth === '0px' && style.overflowY === 'visible';
    }), 'Timeline has no enclosing border or scrolling viewport');
    assert(await page.locator('#thinking-probe .reasoning-step').evaluateAll(steps => steps.every(step => {
      const marker = step.querySelector('.process-marker').getBoundingClientRect();
      const box = step.querySelector('.reasoning-text').getBoundingClientRect();
      return marker.right < box.left && marker.top >= box.top && marker.bottom < box.bottom;
    })), 'Timeline markers sit outside their output boxes');
    await firstViewport.evaluate(el => { el.scrollTop = 40; el.dispatchEvent(new Event('scroll')); });
    await viewport.evaluate(el => { window.originalViewport = el; el.scrollTop = 80; el.dispatchEvent(new Event('scroll')); });
    await page.evaluate(() => window.renderThinking('running', '\n追加的新内容。'.repeat(12)));
    assert.equal(await viewport.evaluate(el => el.scrollTop), 80, 'Reading position stays while receiving text');
    await toggle.click();
    assert(await page.locator('.thinking-collapse').evaluate(el => el.inert));
    await page.evaluate(() => window.renderThinking('running', '\n收起时收到的内容。'.repeat(12)));
    await toggle.click();
    assert(await viewport.evaluate(el => el === window.originalViewport), 'Collapse retains the same viewport');
    assert.equal(await viewport.evaluate(el => el.scrollTop), 80, 'Reopen preserves reading position');
    assert.equal(await firstViewport.evaluate(el => el.scrollTop), 40, 'Each step retains its own reading position');
    assert.equal(normalizeText(await lastText.textContent()), normalizeText(await page.evaluate(() => window.probeText)), 'Reopen does not replay');
    await viewport.evaluate(el => { el.scrollTop = el.scrollHeight; el.dispatchEvent(new Event('scroll')); });
    const outerTop = await page.locator('.chat-scroll').evaluate(el => el.scrollTop);
    await page.evaluate(() => window.renderThinking('running', '\n继续分析新增资料。'.repeat(15)));
    await page.waitForFunction(() => {
      const el = document.querySelector('#thinking-probe .reasoning-step:last-child .reasoning-text');
      return el.scrollHeight - el.clientHeight - el.scrollTop < 2;
    });
    assert.equal(await page.locator('.chat-scroll').evaluate(el => el.scrollTop), outerTop, 'Internal updates do not move the chat');
    assert.equal(await firstViewport.evaluate(el => el.scrollTop), 40, 'Live step updates do not scroll earlier steps');
    await viewport.evaluate(el => { el.scrollTop = 80; el.dispatchEvent(new Event('scroll')); });
    await page.evaluate(() => window.renderThinking('completed'));
    await toggle.click();
    await toggle.click();
    assert.equal(await viewport.evaluate(el => el.scrollTop), 80, 'Completed reasoning retains reading position');
    assert.equal(normalizeText(await lastText.textContent()), normalizeText(await page.evaluate(() => window.probeText)));
    assert.equal(await page.locator('#thinking-probe .process-tool').count(), 1);
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
    await viewport.focus();
    await page.keyboard.press('ArrowDown');
    await page.waitForFunction(() => document.querySelector('#thinking-probe .reasoning-step:last-child .reasoning-text').scrollTop > 80);
    await viewport.evaluate(el => { el.scrollTop = 0; });
    await firstViewport.evaluate(el => { el.scrollTop = 0; });
    await viewport.evaluate(el => el.blur());
    await page.locator('.chat-scroll').evaluate(el => { el.scrollTop = 0; });
    await page.screenshot({ path: `${output}/thinking-${width}-${height}.png` });
    report.checks.push(`${width}x${height}: unframed timeline, per-step bounded boxes, independent retained scroll, instant text, contained updates, tool steps and keyboard scrolling`);
  }
  assert.deepEqual(report.page_errors, []);
  report.passed = true;
} catch (error) {
  report.passed = false;
  report.failure = error.message;
  throw error;
} finally {
  await writeFile(`${output}/report.json`, JSON.stringify(report, null, 2));
  await browser.close();
}
console.log(JSON.stringify(report, null, 2));
