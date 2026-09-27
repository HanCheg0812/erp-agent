import { execFile } from 'node:child_process';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname } from 'node:path';
import { promisify } from 'node:util';
import pg from 'pg';

const run = promisify(execFile);
const intervalMs = Number(process.env.RESOURCE_INTERVAL_MS ?? 500);
const output = process.env.RESOURCE_OUTPUT ?? `/tmp/erp-agent-resources-${Date.now()}.json`;
const metricsOrigin = process.env.RESOURCE_CUBELET_ORIGIN ?? 'http://127.0.0.1:9998';
if (!Number.isInteger(intervalMs) || intervalMs < 100) throw new Error('RESOURCE_INTERVAL_MS must be at least 100');

const db = process.env.DATABASE_URL ? new pg.Client({ connectionString: process.env.DATABASE_URL }) : null;
const samples = [];
const errors = [];
let stopping = false;
let lastCpu;

const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const metricValues = (text, name) => [...text.matchAll(new RegExp(`^${name}\\{[^}]*\\} ([0-9.eE+-]+)$`, 'gm'))]
  .map(match => Number(match[1])).filter(Number.isFinite);
const max = values => values.length ? Math.max(...values) : null;

async function sample() {
  const sampledAt = new Date().toISOString();
  const [stat, meminfo, vmstat, memoryPressure, processList, metrics] = await Promise.all([
    readFile('/proc/stat', 'utf8'),
    readFile('/proc/meminfo', 'utf8'),
    readFile('/proc/vmstat', 'utf8'),
    readFile('/proc/pressure/memory', 'utf8'),
    run('ps', ['-e', '-o', 'comm=,rss=']).then(result => result.stdout),
    fetch(`${metricsOrigin}/v1/metrics/resource`, { signal: AbortSignal.timeout(3000) })
      .then(async response => {
        if (!response.ok) throw new Error(`Cubelet metrics HTTP ${response.status}`);
        return response.text();
      }).catch(error => {
        errors.push({ sampled_at: sampledAt, source: 'cubelet', error: String(error) });
        return '';
      }),
  ]);
  const cpu = stat.split('\n')[0].trim().split(/\s+/).slice(1, 9).map(Number);
  const total = cpu.reduce((sum, value) => sum + value, 0);
  const idle = cpu[3] + cpu[4];
  const busyPercent = lastCpu && total > lastCpu.total
    ? Math.round(1000 * (1 - (idle - lastCpu.idle) / (total - lastCpu.total))) / 10
    : null;
  lastCpu = { total, idle };

  const processes = {};
  for (const line of processList.trim().split('\n')) {
    const match = line.trim().match(/^(\S+)\s+(\d+)$/);
    if (!match) continue;
    const name = match[1];
    if (!['erp-agent-api', 'erp-agent-runti', 'cubelet'].includes(name)) continue;
    const entry = processes[name] ?? { count: 0, rss_mib: 0 };
    entry.count += 1;
    entry.rss_mib += Number(match[2]) / 1024;
    processes[name] = entry;
  }
  for (const entry of Object.values(processes)) entry.rss_mib = Math.round(entry.rss_mib * 10) / 10;

  const available = meminfo.match(/^MemAvailable:\s+(\d+) kB$/m);
  const swapTotal = meminfo.match(/^SwapTotal:\s+(\d+) kB$/m);
  const swapFree = meminfo.match(/^SwapFree:\s+(\d+) kB$/m);
  const vmstatValue = name => Number(vmstat.match(new RegExp(`^${name} (\\d+)$`, 'm'))?.[1] ?? 0);
  const pressureAvg10 = name => Number(memoryPressure.match(new RegExp(`^${name} avg10=([0-9.]+)`, 'm'))?.[1] ?? 0);
  const sandboxMemory = metricValues(metrics, 'cubesandbox_host_sandbox_memory_current_bytes');
  const sandboxCpu = metricValues(metrics, 'cubesandbox_host_sandbox_cpu_usage_seconds_total');
  const guestMemory = metricValues(metrics, 'cubesandbox_guest_workload_memory_current_bytes');
  const guestFailures = metricValues(metrics, 'cubesandbox_guest_workload_memory_failures_total');
  const hostFailures = metricValues(metrics, 'cubesandbox_host_sandbox_memory_failures_total');
  const result = {
    sampled_at: sampledAt,
    host_busy_percent: busyPercent,
    host_available_mib: available ? Math.round(Number(available[1]) / 1024) : null,
    host_swap_used_mib: swapTotal && swapFree ? Math.round((Number(swapTotal[1]) - Number(swapFree[1])) / 1024) : null,
    host_swap_in_pages: vmstatValue('pswpin'),
    host_swap_out_pages: vmstatValue('pswpout'),
    host_memory_some_avg10: pressureAvg10('some'),
    host_memory_full_avg10: pressureAvg10('full'),
    processes,
    cubelet: {
      sandbox_series: sandboxMemory.length,
      sandbox_memory_mib: Math.round(sandboxMemory.reduce((sum, value) => sum + value, 0) / 1048576),
      sandbox_cpu_seconds_total: sandboxCpu.reduce((sum, value) => sum + value, 0),
      sandbox_memory_failures: hostFailures.reduce((sum, value) => sum + value, 0),
      guest_series: guestMemory.length,
      guest_memory_mib: Math.round(guestMemory.reduce((sum, value) => sum + value, 0) / 1048576),
      guest_max_memory_mib: guestMemory.length ? Math.round(max(guestMemory) / 1048576) : null,
      guest_memory_failures: guestFailures.reduce((sum, value) => sum + value, 0),
    },
  };
  if (db && samples.length % Math.max(1, Math.ceil(1000 / intervalMs)) === 0) {
    try {
      result.postgres_connections = Number((await db.query(
        'SELECT count(*)::int AS count FROM pg_stat_activity WHERE datname=current_database()',
      )).rows[0].count);
    } catch (error) {
      errors.push({ sampled_at: sampledAt, source: 'postgres', error: String(error) });
    }
  }
  samples.push(result);
}

process.on('SIGINT', () => { stopping = true; });
process.on('SIGTERM', () => { stopping = true; });
if (db) await db.connect();
console.log(`Sampling resources every ${intervalMs} ms; output: ${output}`);
try {
  while (!stopping) {
    try { await sample(); } catch (error) { errors.push({ sampled_at: new Date().toISOString(), source: 'sample', error: String(error) }); }
    if (!stopping) await sleep(intervalMs);
  }
} finally {
  if (db) await db.end();
  const report = {
    interval_ms: intervalMs,
    scope: 'Node-wide samples; host_sandbox memory is a cgroup charge, not guest working set. Guest values are null when Cubelet does not export guest_workload.',
    samples,
    errors,
    summary: {
      max_host_busy_percent: max(samples.map(item => item.host_busy_percent).filter(Number.isFinite)),
      min_host_available_mib: samples.length ? Math.min(...samples.map(item => item.host_available_mib).filter(Number.isFinite)) : null,
      max_api_rss_mib: max(samples.map(item => item.processes['erp-agent-api']?.rss_mib ?? 0)),
      max_runtime_rss_mib: max(samples.map(item => item.processes['erp-agent-runti']?.rss_mib ?? 0)),
      max_cube_sandbox_series: max(samples.map(item => item.cubelet.sandbox_series)),
      max_cube_sandbox_memory_mib: max(samples.map(item => item.cubelet.sandbox_memory_mib)),
      max_cube_sandbox_memory_failures: max(samples.map(item => item.cubelet.sandbox_memory_failures)),
      max_cube_guest_series: max(samples.map(item => item.cubelet.guest_series)),
      max_cube_guest_memory_mib: max(samples.map(item => item.cubelet.guest_series ? item.cubelet.guest_memory_mib : null).filter(Number.isFinite)),
      max_cube_guest_instance_memory_mib: max(samples.map(item => item.cubelet.guest_max_memory_mib).filter(Number.isFinite)),
      max_cube_guest_memory_failures: max(samples.map(item => item.cubelet.guest_series ? item.cubelet.guest_memory_failures : null).filter(Number.isFinite)),
      max_host_swap_used_mib: max(samples.map(item => item.host_swap_used_mib).filter(Number.isFinite)),
      host_swap_in_pages_delta: samples.length ? samples.at(-1).host_swap_in_pages - samples[0].host_swap_in_pages : null,
      host_swap_out_pages_delta: samples.length ? samples.at(-1).host_swap_out_pages - samples[0].host_swap_out_pages : null,
      max_host_memory_some_avg10: max(samples.map(item => item.host_memory_some_avg10)),
      max_host_memory_full_avg10: max(samples.map(item => item.host_memory_full_avg10)),
      max_postgres_connections: max(samples.map(item => item.postgres_connections).filter(Number.isFinite)),
    },
  };
  await mkdir(dirname(output), { recursive: true });
  await writeFile(output, JSON.stringify(report, null, 2));
  console.log(JSON.stringify({ output, samples: samples.length, summary: report.summary, errors: errors.length }));
}
