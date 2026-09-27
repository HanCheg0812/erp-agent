import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { mkdir, writeFile } from 'node:fs/promises';

const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright');
const origin = process.env.VERIFY_WEB_ORIGIN ?? 'http://127.0.0.1:4173';
const output = process.env.VERIFY_AUTH_OUTPUT ?? '/tmp/erp-auth-entry';
const tokenKey = 'erp-agent-token';
const suffix = randomUUID();
const account = { email: `web-auth-${suffix}@example.test`, password: `Test-${randomUUID()}` };
const second = { email: `web-auth-other-${suffix}@example.test`, password: `Test-${randomUUID()}` };
const report = { origin, checked_at: new Date().toISOString(), checks: [], screenshots: [], page_errors: [] };
await mkdir(output, { recursive: true });
const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH ?? '/usr/bin/google-chrome', headless: true });
const context = await browser.newContext({ viewport: { width: 1440, height: 1000 } });
context.on('page', tab => tab.on('pageerror', error => report.page_errors.push(error.message)));
const page = await context.newPage();
page.setDefaultTimeout(15_000);

async function login(tab, credentials = account) {
  await tab.getByLabel('Email', { exact: true }).fill(credentials.email);
  await tab.getByLabel('Password', { exact: true }).fill(credentials.password);
  await tab.getByRole('button', { name: 'Sign in', exact: true }).click();
  await tab.locator('.chat-app').waitFor({ state: 'attached' });
}

async function register(tab, credentials) {
  await tab.getByRole('button', { name: 'Create an account', exact: true }).click();
  await tab.getByLabel('Email', { exact: true }).fill(credentials.email);
  await tab.getByLabel('Password', { exact: true }).fill(credentials.password);
  await tab.getByRole('button', { name: 'Create account', exact: true }).click();
  await tab.getByRole('heading', { name: 'No conversations yet', exact: true }).waitFor();
}

async function showSidebar(tab) {
  if (!await tab.getByRole('button', { name: 'Sign out', exact: true }).isVisible()) {
    await tab.getByRole('button', { name: 'Open conversations', exact: true }).click();
  }
}

async function assertSignedOut(tab) {
  await tab.getByRole('heading', { name: 'Sign in to Forge', exact: true }).waitFor();
  assert.equal(await tab.locator('.chat-app').count(), 0, 'Workspace must unmount after sign-out');
  assert.equal(await tab.evaluate(key => localStorage.getItem(key), tokenKey), null);
}

async function screenshot(name) {
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth > innerWidth);
  assert.equal(overflow, false, `${name}: horizontal overflow`);
  await page.screenshot({ path: `${output}/${name}.png`, fullPage: true });
  report.screenshots.push(name);
}

async function revoke(tab) {
  const token = await tab.evaluate(key => localStorage.getItem(key), tokenKey);
  assert(token);
  const response = await tab.request.post(`${origin}/v1/auth/logout`, { headers: { authorization: `Bearer ${token}` } });
  assert.equal(response.status(), 204, 'Rust API must revoke the session');
}

try {
  await page.goto(origin);
  await assertSignedOut(page);
  assert.equal(await page.getByRole('button', { name: 'View demo', exact: true }).count(), 0);
  for (const width of [1440, 390, 320]) {
    await page.setViewportSize({ width, height: width === 1440 ? 1000 : 844 });
    await screenshot(`login-${width}`);
    await page.getByRole('button', { name: 'Create an account', exact: true }).click();
    await screenshot(`register-${width}`);
    await page.getByRole('button', { name: 'Sign in', exact: true }).click();
  }
  await page.setViewportSize({ width: 390, height: 400 });
  await page.getByLabel('Password', { exact: true }).fill('test');
  await page.getByRole('button', { name: 'Sign in', exact: true }).scrollIntoViewIfNeeded();
  await page.getByRole('button', { name: 'Sign in', exact: true }).click({ trial: true });
  const submitBox = await page.getByRole('button', { name: 'Sign in', exact: true }).boundingBox();
  assert(submitBox && submitBox.y >= -1 && submitBox.y + submitBox.height <= 401, 'Submit is within one pixel of the viewport after scrolling');
  await screenshot('login-short-viewport');
  report.checks.push('Fresh entry is auth at 1440/390/320; short viewport submit remains reachable');

  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.getByLabel('Email', { exact: true }).fill(account.email);
  await page.getByRole('button', { name: 'Sign in', exact: true }).click();
  await page.getByRole('alert').filter({ hasText: 'Email or password is incorrect.' }).waitFor();
  await assertSignedOut(page);
  report.checks.push('Incorrect credentials show a readable error and do not authenticate');

  report.checks.push('Unauthenticated entry has no demo session or simulated business data');

  await page.setViewportSize({ width: 390, height: 844 });
  await register(page, account);
  await screenshot('registered-empty-mobile');
  await showSidebar(page);
  await page.getByRole('button', { name: 'Personal memory', exact: true }).click();
  await page.getByText('No saved memories.', { exact: true }).waitFor();
  await page.getByRole('button', { name: 'Close memory', exact: true }).click();
  await page.getByRole('complementary', { name: 'Case conversations' }).getByRole('button', { name: 'Close conversations', exact: true }).click();
  report.checks.push('Rust registration creates an empty personal workspace and memory; mobile account menu is reachable');

  await page.getByRole('button', { name: 'Start a conversation', exact: true }).click();
  const caseTitle = 'New conversation';
  await page.getByRole('heading', { name: caseTitle, exact: true }).waitFor();
  assert.equal(await page.getByRole('dialog').count(), 0, 'Starting a conversation must not require a case form');
  await page.reload();
  await page.getByRole('heading', { name: caseTitle, exact: true }).waitFor();
  await screenshot('authenticated-mobile');
  await page.setViewportSize({ width: 1440, height: 1000 });
  await screenshot('authenticated-desktop');
  const token = await page.evaluate(key => localStorage.getItem(key), tokenKey);
  const cases = await page.request.get(`${origin}/v1/cases`, { headers: { authorization: `Bearer ${token}` } });
  assert.equal(cases.status(), 200);
  const items = (await cases.json()).items;
  assert.equal(items.length, 1);
  report.case_id = items[0].id;
  assert.equal(items[0].kind, 'conversation');
  assert.equal(items[0].customer_code, null);
  report.checks.push('New conversation needs no case form; Rust persists it and refreshing restores login and the same conversation');

  const tab = await context.newPage();
  await tab.goto(origin);
  await tab.getByRole('heading', { name: caseTitle, exact: true }).waitFor();
  await page.getByRole('button', { name: 'Sign out', exact: true }).click();
  await assertSignedOut(page);
  await assertSignedOut(tab);
  const revoked = await page.request.get(`${origin}/v1/auth/me`, { headers: { authorization: `Bearer ${token}` } });
  assert.equal(revoked.status(), 401);
  report.checks.push('New tab restores session; logout revokes server token and clears both tabs');

  await login(page);
  await tab.getByRole('heading', { name: caseTitle, exact: true }).waitFor();
  await revoke(page);
  await page.getByRole('button', { name: 'Refresh cases', exact: true }).click();
  await assertSignedOut(page);
  await assertSignedOut(tab);
  await page.getByRole('alert').filter({ hasText: 'Your session has expired.' }).waitFor();
  await screenshot('expired-session');
  report.checks.push('401 during use returns both tabs to auth and displays expiration notice');
  await tab.close();

  await login(page);
  await revoke(page);
  await page.reload();
  await assertSignedOut(page);
  await page.getByRole('alert').filter({ hasText: 'Your session has expired.' }).waitFor();
  report.checks.push('Reload with a revoked token returns to auth without showing demo cases');

  await login(page);
  await page.route('**/v1/auth/me', route => route.fulfill({ status: 503, contentType: 'application/json', body: JSON.stringify({ message: 'Temporary API outage' }) }));
  await page.reload();
  await page.getByRole('heading', { name: 'Service unavailable', exact: true }).waitFor();
  assert.equal(await page.getByText('Demo session', { exact: true }).count(), 0);
  assert(await page.evaluate(key => localStorage.getItem(key), tokenKey));
  await page.unroute('**/v1/auth/me');
  await page.getByRole('button', { name: 'Retry', exact: true }).click();
  await page.getByRole('heading', { name: caseTitle, exact: true }).waitFor();
  report.checks.push('Transient server failure preserves the session; Retry restores real data without demo fallback');

  await page.getByRole('button', { name: 'Sign out', exact: true }).click();
  await assertSignedOut(page);
  const accountTab = await context.newPage();
  await accountTab.goto(origin);
  await register(page, second);
  await accountTab.getByRole('heading', { name: 'No conversations yet', exact: true }).waitFor();
  assert.equal(await page.getByRole('heading', { name: caseTitle, exact: true }).count(), 0);
  assert.equal(await accountTab.getByRole('heading', { name: caseTitle, exact: true }).count(), 0);
  const secondToken = await page.evaluate(key => localStorage.getItem(key), tokenKey);
  const crossAccount = await page.request.get(`${origin}/v1/cases/${report.case_id}`, { headers: { authorization: `Bearer ${secondToken}` } });
  assert([403, 404].includes(crossAccount.status()), 'Second account must not access first account case');
  report.checks.push('Switching accounts clears both tabs; Rust denies access to the previous account case');
  await page.getByRole('button', { name: 'Sign out', exact: true }).click();
  await assertSignedOut(page);
  assert.deepEqual(report.page_errors, []);
  report.passed = true;
} catch (error) {
  report.passed = false;
  report.failure = error.message;
  await page.screenshot({ path: `${output}/failure.png`, fullPage: true }).catch(() => {});
  throw error;
} finally {
  await writeFile(`${output}/report.json`, `${JSON.stringify(report, null, 2)}\n`);
  await browser.close();
}
console.log(`PASS ${report.checks.length} auth workflow checks; screenshots and report: ${output}`);
