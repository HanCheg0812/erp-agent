import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import pg from 'pg';

const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright');
const origin = process.env.VERIFY_WEB_ORIGIN ?? 'http://127.0.0.1:4174';
const output = process.env.VERIFY_RUNTIME_OUTPUT ?? `/tmp/erp-runtime-management/${Date.now()}`;
const report = { started_at: new Date().toISOString(), checks: [], screenshots: [], page_errors: [], cleanup: [] };
const db = new pg.Client({ connectionString: process.env.DATABASE_URL });
await db.connect();
await mkdir(output, { recursive: true });
const browser = await chromium.launch({ executablePath: '/usr/bin/google-chrome', headless: true });
const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
await page.emulateMedia({ reducedMotion: 'reduce' });
page.setDefaultTimeout(20_000);
page.on('pageerror', error => report.page_errors.push(error.message));
let token;
const cases = new Set();
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
async function api(path, method = 'GET', data) {
  const response = await page.request.fetch(`${origin}/v1${path}`, {
    method, headers: { authorization: `Bearer ${token}` }, ...(data === undefined ? {} : { data }),
  });
  assert(response.ok(), `${method} ${path}: HTTP ${response.status()}`);
  return response.json();
}
async function capture(name) {
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false, `${name}: page overflow`);
  const surface = await page.locator('.runtime-page').count() ? await page.locator('.runtime-page').boundingBox() : null;
  if (surface) assert(surface.x >= 0 && surface.x + surface.width <= page.viewportSize().width + 1, `${name}: runtime surface overflow`);
  await page.screenshot({ path: `${output}/${name}.png`, animations: 'disabled' });
  report.screenshots.push(`${name}.png`);
}
async function runtime() {
  await page.getByRole('button', { name: 'Runtime', exact: true }).click();
  await page.getByRole('heading', { name: 'Runtime management' }).waitFor();
  await page.getByTitle('Refresh runtime status').waitFor({ state: 'visible' });
}
async function waitCandidate(id) {
  const deadline = Date.now() + 240_000;
  let previous;
  while (Date.now() < deadline) {
    const candidate = await api(`/runtime/candidates/${id}`);
    if (candidate.state !== previous) { console.log(`Candidate ${id}: ${candidate.state}`); previous = candidate.state; }
    if (candidate.state === 'ready') return candidate;
    assert(['requested', 'building'].includes(candidate.state), `Build ended in ${candidate.state}: ${candidate.error_code}`);
    await sleep(2000);
  }
  throw Error('Cube template build deadline exceeded');
}
async function send(caseId, text) {
  await page.getByRole('textbox', { name: 'Message agent' }).fill(text);
  const response = page.waitForResponse(r => r.url().endsWith('/conversation/messages') && r.request().method() === 'POST');
  await page.getByRole('button', { name: 'Send message', exact: true }).click();
  const submitted = await response;
  assert.equal(submitted.status(), 201);
  const { message } = await submitted.json();
  const deadline = Date.now() + 180_000;
  while (Date.now() < deadline) {
    const history = await api(`/cases/${caseId}/conversation`);
    const result = history.items.find(item => item.id === message.id)?.response;
    if (result && ['completed', 'failed', 'cancelled'].includes(result.state)) {
      assert.equal(result.state, 'completed', `Agent turn: ${JSON.stringify(result.error)}`);
      const timing = (await db.query('SELECT state,timings FROM agent_runs WHERE message_id=$1', [message.id])).rows[0];
      assert.equal(timing.timings.at(-1).backend, 'cube');
      console.log(`Cube turn ${message.id}: completed`);
      return { message_id: message.id, text: result.text, tools: result.tools, ...timing };
    }
    await sleep(1000);
  }
  throw Error('Cube Agent turn deadline exceeded');
}
async function binding(caseId) {
  const row = (await db.query('SELECT id,state,sandbox_id,last_sandbox_id,runtime_template_id,runtime_candidate_id,runtime_release_id FROM case_sandboxes WHERE case_id=$1 ORDER BY created_at DESC LIMIT 1', [caseId])).rows[0];
  const response = await fetch(`${process.env.CUBE_API_URL.replace(/\/$/, '')}/sandboxes/${row.sandbox_id}`, {
    headers: process.env.CUBE_API_KEY ? { 'X-API-Key': process.env.CUBE_API_KEY } : {},
  });
  assert.equal(response.status, 200);
  const instance = await response.json();
  assert.equal(instance.templateID, row.runtime_template_id, 'Cube inventory must match the persisted template binding');
  return { ...row, observed_template_id: instance.templateID };
}
try {
  await page.goto(origin);
  await page.getByLabel('Email', { exact: true }).fill(process.env.DEMO_EMAIL);
  await page.getByLabel('Password', { exact: true }).fill(process.env.DEMO_PASSWORD);
  await page.getByRole('button', { name: 'Sign in', exact: true }).click();
  await page.locator('.chat-app').waitFor();
  token = await page.evaluate(() => localStorage.getItem('erp-agent-token'));
  await runtime();
  const initial = await api('/runtime');
  assert(initial.provider_available && initial.nodes.some(node => node.healthy));
  report.initial_version = initial.version;
  report.nodes = initial.nodes;
  let candidate;
  if (process.env.VERIFY_RUNTIME_CANDIDATE) {
    candidate = await api(`/runtime/candidates/${process.env.VERIFY_RUNTIME_CANDIDATE}`);
    await page.locator('.runtime-candidate').filter({ hasText: candidate.name }).click();
  } else {
    const image = initial.templates.find(template => template.template_id === initial.legacy_template_id)?.image;
    assert(image?.includes('@sha256:'), 'Legacy template must have a pinned runtime image');
    const name = `Web runtime validation ${Date.now()}`;
    await page.getByRole('button', { name: 'New candidate', exact: true }).click();
    await page.getByLabel('Name', { exact: true }).fill(name);
    await page.getByLabel('OCI image with SHA-256 digest', { exact: true }).fill(image);
    const saved = page.waitForResponse(r => r.url().endsWith('/runtime/candidates') && r.request().method() === 'POST');
    await page.getByRole('button', { name: 'Save candidate', exact: true }).click();
    const response = await saved;
    assert.equal(response.status(), 201);
    candidate = await response.json();
  }
  report.candidate_id = candidate.id;
  report.image = candidate.image;
  await page.locator('.runtime-inspector h3').filter({ hasText: candidate.name }).waitFor();
  if (candidate.build_request) {
    await page.getByText('Cube build request', { exact: true }).click();
    assert.equal(candidate.build_request.allowInternetAccess, false);
    assert.deepEqual(candidate.build_request.command, ['/usr/local/bin/resident-start.sh']);
  } else {
    assert.equal(candidate.source, 'adopt');
    assert.equal(candidate.state, 'ready');
  }
  await capture('desktop-build-preview');
  if (candidate.state === 'draft') {
    const built = page.waitForResponse(r => r.url().endsWith(`/runtime/candidates/${candidate.id}/build`));
    await page.getByRole('button', { name: 'Build template', exact: true }).click();
    assert.equal((await built).status(), 202);
  }
  const buildStarted = Date.now();
  candidate = await waitCandidate(candidate.id);
  report.build_wait_ms = Date.now() - buildStarted;
  report.template_id = candidate.template_id;
  report.job_id = candidate.job_id;
  report.checks.push('Real Web candidate and restricted Cube build reach READY');
  await page.getByTitle('Refresh runtime status').click();
  const opened = page.waitForResponse(r => r.url().endsWith(`/runtime/candidates/${candidate.id}/canary`));
  await page.getByRole('button', { name: /^(Create|Open) test conversation$/ }).click();
  const canaryResponse = await opened;
  assert.equal(canaryResponse.status(), 201);
  const { case_id: canaryId } = await canaryResponse.json();
  cases.add(canaryId);
  report.canary_case_id = canaryId;
  await page.getByRole('textbox', { name: 'Message agent' }).waitFor();
  await page.getByRole('button', { name: 'Model and reasoning' }).click();
  await page.getByRole('button', { name: 'Disable reasoning', exact: true }).click();
  await page.keyboard.press('Escape');
  report.canary_turn = await send(canaryId, 'Call case_read once to read this current conversation. Then briefly report its title. Do not call other tools. This is a synthetic runtime validation.');
  assert(report.canary_turn.tools.some(tool => tool.name === 'case_read' && tool.state === 'completed'));
  report.canary_binding = await binding(canaryId);
  assert.equal(report.canary_binding.runtime_template_id, candidate.template_id);
  assert.equal(report.canary_binding.runtime_candidate_id, candidate.id);
  assert(report.canary_binding.last_sandbox_id);
  assert((await api(`/runtime/candidates/${candidate.id}`)).canary.ready);
  await capture('desktop-canary');
  report.checks.push('Real JCode case_read turn and exact Cube template binding persist in PostgreSQL');
  await runtime();
  const active = initial.releases.find(value => value.id === initial.active_release_id);
  if (active?.candidate_id === candidate.id) {
    report.release = { release_id: active.id, candidate_id: candidate.id, template_id: active.template_id, version: initial.version };
    report.existing_release = true;
  } else {
    const activated = page.waitForResponse(r => r.url().endsWith(`/runtime/candidates/${candidate.id}/activate`));
    await page.getByRole('button', { name: 'Activate', exact: true }).click();
    const activation = await activated;
    assert.equal(activation.status(), 200);
    report.release = await activation.json();
    assert.equal(report.release.version, initial.version + 1);
  }
  await page.getByRole('button', { name: 'Back to conversations', exact: true }).click();
  const newCase = page.waitForResponse(r => r.url().endsWith('/v1/conversations') && r.request().method() === 'POST');
  await page.getByRole('button', { name: 'New conversation', exact: true }).click();
  const ordinaryId = (await (await newCase).json()).id;
  cases.add(ordinaryId);
  report.released_case_id = ordinaryId;
  await page.getByRole('textbox', { name: 'Message agent' }).waitFor();
  report.released_turn = await send(ordinaryId, 'Reply with only the number 42. Do not call any tools.');
  report.released_binding = await binding(ordinaryId);
  assert.equal(report.released_binding.runtime_release_id, report.release.release_id);
  assert.equal(report.released_binding.runtime_template_id, candidate.template_id);
  report.followup_turn = await send(ordinaryId, 'What number did you just answer? Reply with that number only. Do not call tools.');
  assert(report.followup_turn.text.includes('42'));
  assert.deepEqual(await binding(ordinaryId), report.released_binding);
  report.checks.push('Activation changes new sandbox selection; a second round reuses the same bound instance');
  await runtime();
  for (const width of [1440, 390, 320]) {
    await page.setViewportSize({ width, height: width === 1440 ? 1000 : 844 });
    for (const tab of ['Templates', 'Instances', 'Nodes', 'Releases']) {
      await page.getByRole('tab', { name: tab, exact: true }).click();
      await capture(`${tab.toLowerCase()}-${width}`);
    }
  }
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.route('**/v1/runtime', route => route.fulfill({ status: 503, contentType: 'application/json', body: JSON.stringify({ error: 'Runtime temporarily unavailable' }) }), { times: 1 });
  await page.getByTitle('Refresh runtime status').click();
  await page.getByRole('alert').waitFor();
  await capture('runtime-error-retry');
  await page.getByRole('button', { name: 'Retry', exact: true }).click();
  await page.getByRole('alert').waitFor({ state: 'hidden' });
  report.checks.push('Four admin views fit desktop/390/320 px; transient HTTP error recovers with Retry');
  assert.deepEqual(report.page_errors, []);
  report.passed = true;
} catch (error) {
  report.passed = false;
  report.failure = error.message;
  await page.screenshot({ path: `${output}/failure.png` }).catch(() => {});
  process.exitCode = 1;
} finally {
  for (const caseId of cases) {
    try {
      const history = await api(`/cases/${caseId}/conversation`);
      for (const item of history.items.filter(item => ['queued', 'running', 'waiting'].includes(item.response?.state))) {
        await api(`/cases/${caseId}/conversation/cancel`, 'POST', { message_id: String(item.id) });
      }
      const state = await api(`/cases/${caseId}/sandbox`);
      if (state.operation && state.operation.state !== 'released') {
        const released = await api(`/cases/${caseId}/sandbox/${state.operation.id}`, 'DELETE');
        report.cleanup.push({ case_id: caseId, state: released.operation?.state });
      }
    } catch (error) { report.cleanup.push({ case_id: caseId, error: error.message }); report.passed = false; process.exitCode = 1; }
  }
  report.finished_at = new Date().toISOString();
  await writeFile(`${output}/report.json`, JSON.stringify(report, null, 2));
  await browser.close();
  await db.end();
}
console.log(JSON.stringify({ passed: report.passed, failure: report.failure, candidate_id: report.candidate_id, template_id: report.template_id, checks: report.checks, report: `${output}/report.json` }, null, 2));
