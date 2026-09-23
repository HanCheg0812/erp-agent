import assert from 'node:assert/strict';
import { randomBytes, randomUUID } from 'node:crypto';
import { mkdir, writeFile } from 'node:fs/promises';
import pg from 'pg';

const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright');
const origin = process.env.VERIFY_WEB_ORIGIN ?? 'http://127.0.0.1:4173';
const output = process.env.VERIFY_REASONING_OUTPUT ?? '/tmp/erp-reasoning-settings';
await mkdir(output, { recursive: true });
const report = { checks: [], turns: [], page_errors: [] };
let token;
let caseId;

async function api(path, body, method = body ? 'POST' : 'GET') {
  const response = await fetch(`${origin}/v1${path}`, {
    method,
    headers: { 'content-type': 'application/json', ...(token ? { authorization: `Bearer ${token}` } : {}) },
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

async function completed(messageId) {
  for (let attempt = 0; attempt < 300; attempt++) {
    const history = await api(`/cases/${caseId}/conversation`);
    const response = history.items.find(item => item.id === messageId)?.response;
    assert.notEqual(response?.state, 'failed', JSON.stringify(response));
    if (response?.state === 'completed') return { history, response };
    await new Promise(resolve => setTimeout(resolve, 600));
  }
  throw Error('Reasoning verification wait expired');
}

async function setEffort(effort) {
  const settings = { approval: 'ask', model: 'deepseek-flash', effort };
  const result = await api(`/cases/${caseId}/conversation/settings`, settings, 'PATCH');
  assert.equal(result.settings.effort, effort);
  return settings;
}

async function runTurn(effort) {
  const settings = await setEffort(effort);
  const message = await api(`/cases/${caseId}/conversation/messages`, {
    client_message_id: randomUUID(),
    text: 'Solve this carefully and return only the final answer as a single integer. A factory has 7 identical production lines. Each line makes 18 units in the first hour, then 3 fewer units every following hour. Production runs for 5 hours. Of the total, 12% is rejected, rounded down to a whole unit, and the accepted units are packed equally into 6 crates. How many accepted units go in each crate? Do not call tools.',
    settings,
  });
  const { history, response } = await completed(message.message.id);
  const run = await db.query('SELECT settings, steps, state FROM agent_runs WHERE message_id=$1', [message.message.id]);
  assert.equal(run.rows.length, 1);
  assert.equal(run.rows[0].state, 'completed');
  assert.equal(run.rows[0].settings.effort, effort);
  const reasoning = response.steps.filter(step => step.kind === 'reasoning');
  const storedReasoning = run.rows[0].steps.filter(step => step.kind === 'reasoning');
  assert.equal(storedReasoning.length, reasoning.length);
  if (effort === 'none') assert.equal(reasoning.length, 0, 'disabled reasoning must not emit reasoning steps');
  else assert(reasoning.length > 0 && reasoning.every(step => step.text.trim()), `${effort} must emit non-empty reasoning steps`);
  report.turns.push({ effort, message_id: message.message.id, response: response.text, reasoning_steps: reasoning.length, reasoning_chars: reasoning.reduce((sum, step) => sum + step.text.length, 0), session_id: history.session_id });
  report.checks.push(`${effort} is persisted and produces the expected reasoning stream`);
}

try {
  token = (await api('/auth/register', { email: `reasoning-${randomUUID()}@example.test`, password: randomBytes(24).toString('hex') })).access_token;
  await page.goto(origin);
  await page.evaluate(value => localStorage.setItem('erp-agent-token', value), token);
  caseId = (await api('/conversations', { request_id: randomUUID() })).id;
  report.case_id = caseId;
  await page.reload();
  await page.getByRole('button', { name: 'Model and reasoning' }).waitFor();

  for (const effort of ['none', 'low', 'high']) await runTurn(effort);
  assert.equal(new Set(report.turns.map(turn => turn.session_id)).size, 1);

  await page.reload();
  await page.getByRole('button', { name: 'Model and reasoning' }).waitFor();
  assert.match(await page.getByRole('button', { name: 'Model and reasoning' }).textContent(), /高/);
  const thought = page.locator('.thinking-process').last();
  await thought.waitFor();
  assert.match(await thought.locator('.thinking-toggle').textContent(), /Thought process/);
  assert.equal(await thought.locator('.reasoning-text').count() > 0, true);
  if (await thought.locator('.thinking-toggle').getAttribute('aria-expanded') !== 'true') await thought.locator('.thinking-toggle').click();
  assert((await thought.locator('.reasoning-text').allTextContents()).join('').trim().length > 0);
  await page.screenshot({ path: `${output}/reasoning-high-expanded.png`, fullPage: true });
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
  report.checks.push('High effort is visible in the model control and expandable Thought process');
  assert.deepEqual(report.page_errors, []);
  report.passed = true;
} catch (error) {
  report.passed = false;
  report.failure = error.message;
  await page.screenshot({ path: `${output}/failure.png`, fullPage: true }).catch(() => {});
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
console.log(`PASS ${report.checks.length} reasoning checks; screenshots: ${output}`);
