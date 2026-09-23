import { randomUUID } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { performance } from 'node:perf_hooks';

const api = 'http://127.0.0.1:3001';
const proxy = 'http://127.0.0.1';
const templateID = 'tpl-erp-agent-base-20260921';
const levels = [1, 2, 4, 8];
const repetitions = 3;
const runID = `erp-agent-cube-bench-${randomUUID()}`;
const evidencePath = new URL(`../../docs/plantree/plans/server-demo/cube-concurrency-evidence-${runID}.json`, import.meta.url);
let interrupted = false;

for (const signal of ['SIGINT', 'SIGTERM']) {
  process.on(signal, () => { interrupted = true; });
}

const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const ms = start => Math.round((performance.now() - start) * 10) / 10;

async function request(path, { method = 'GET', body, timeout = 30000, base = api } = {}) {
  const response = await fetch(`${base}${path}`, {
    method,
    headers: body ? { 'content-type': 'application/json' } : {},
    body: body ? JSON.stringify(body) : undefined,
    signal: AbortSignal.timeout(timeout),
  });
  if (response.status === 404 && method === 'GET') return null;
  if (!response.ok) throw new Error(`${method} ${path} returned HTTP ${response.status}`);
  if (response.status === 204) return null;
  return response.json();
}

async function owned() {
  const items = await request(`/sandboxes?metadata=probe_run%3D${encodeURIComponent(runID)}&limit=1000`);
  if (!Array.isArray(items)) throw new Error('Unexpected sandbox inventory');
  return items.filter(item => item.metadata?.probe_run === runID);
}

async function cleanup() {
  const started = performance.now();
  const errors = [];
  let remaining = [];
  for (let attempt = 0; attempt < 8; attempt++) {
    remaining = await owned();
    if (remaining.length === 0) break;
    const deleted = await Promise.allSettled(remaining.map(item =>
      request(`/sandboxes/${encodeURIComponent(item.sandboxID)}`, { method: 'DELETE', timeout: 10000 })
    ));
    for (const result of deleted) {
      if (result.status === 'rejected') errors.push(String(result.reason));
    }
    await sleep(750);
  }
  remaining = await owned();
  return { releaseMs: ms(started), remaining: remaining.length, errors };
}

async function create(round, slot) {
  const started = performance.now();
  const result = await request('/sandboxes', {
    method: 'POST',
    body: {
      templateID,
      timeout: 900,
      allow_internet_access: false,
      metadata: { project: 'erp-agent', probe_run: runID, probe_round: round, probe_slot: String(slot) },
    },
  });
  if (!result?.sandboxID) {
    throw new Error('Create response has no sandbox ID');
  }
  const createMs = ms(started);
  let ready = false;
  while (ms(started) < 15000) {
    try {
      const health = await request(`/sandbox/${encodeURIComponent(result.sandboxID)}/49999/health`, {
        base: proxy, timeout: 3000,
      });
      if (health === 'OK') { ready = true; break; }
    } catch { /* The proxy route may appear after the create response. */ }
    await sleep(200);
  }
  return { createMs, readyMs: ms(started), ready };
}

async function hostMemory() {
  const text = await readFile('/proc/meminfo', 'utf8');
  return Object.fromEntries(['MemAvailable', 'SwapFree'].map(key => [
    key, Math.round(Number(text.match(new RegExp(`^${key}:\\s+(\\d+)`, 'm'))?.[1]) / 1024),
  ]));
}

const report = {
  at: new Date().toISOString(),
  runID,
  templateID,
  levels,
  repetitions,
  rounds: [],
};

try {
  const initial = await request('/sandboxes?limit=1000');
  if (!Array.isArray(initial) || initial.length !== 0) {
    throw new Error('Other Cube instances exist; not starting a shared-node load probe');
  }
  const [nodes, template] = await Promise.all([
    request('/cubeapi/v1/nodes'), request(`/templates/${templateID}`),
  ]);
  const healthy = nodes.filter(node => node.healthy);
  if (healthy.length !== 1 || template?.status !== 'READY' ||
      !template.replicas?.some(replica => replica.status === 'READY' && replica.node_ip === healthy[0].hostIP)) {
    throw new Error('Expected one healthy node with the ready local template');
  }
  report.node = {
    hostIP: healthy[0].hostIP,
    quotaCpu: healthy[0].quotaCpu,
    quotaMemMB: healthy[0].quotaMemMB,
    maxMvmSlots: healthy[0].maxMvmSlots,
    templateSpec: template.replicas.find(replica => replica.node_ip === healthy[0].hostIP)?.spec,
  };
  report.hostMemoryBeforeMiB = await hostMemory();
  console.log(`Cube probe ${runID}: empty node, ${report.node.templateSpec}`);

  for (const level of levels) {
    for (let repeat = 1; repeat <= repetitions && !interrupted; repeat++) {
      const roundID = `${level}-${repeat}`;
      const started = performance.now();
      const results = await Promise.allSettled(Array.from({ length: level }, (_, slot) => create(roundID, slot)));
      const values = results.filter(result => result.status === 'fulfilled').map(result => result.value);
      const errors = results.filter(result => result.status === 'rejected').map(result => String(result.reason));
      const inventory = await owned();
      const memoryAtPeakMiB = await hostMemory();
      const release = await cleanup();
      const round = {
        level, repeat, elapsedMs: ms(started), instancesSeen: inventory.length,
        ready: values.filter(value => value.ready).length,
        createsMs: values.map(value => value.createMs),
        readyMs: values.map(value => value.readyMs),
        memoryAtPeakMiB, ...release, errors,
      };
      report.rounds.push(round);
      console.log(JSON.stringify(round));
      if (errors.length || round.ready !== level || inventory.length !== level || release.remaining) {
        report.stopped = `Unsuccessful round ${roundID}`;
        interrupted = true;
      }
    }
  }
} catch (error) {
  report.stopped = String(error);
} finally {
  // A timed-out create may finish after its HTTP client has gone away.
  await sleep(2000);
  try { report.finalCleanup = await cleanup(); }
  catch (error) { report.finalCleanup = { error: String(error) }; }
  report.hostMemoryAfterMiB = await hostMemory();
  await writeFile(evidencePath, `${JSON.stringify(report, null, 2)}\n`, { flag: 'wx' });
  console.log(`Evidence: ${evidencePath.pathname}`);
  if (report.stopped || report.finalCleanup.remaining || report.finalCleanup.error) {
    console.error(report.stopped ?? 'Owned instances remain after cleanup');
    process.exitCode = 1;
  }
}
