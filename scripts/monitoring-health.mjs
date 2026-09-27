import { execFileSync } from 'node:child_process';
import { mkdir, rename, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const root = resolve(import.meta.dirname, '..');
const textfileDir = resolve(root, process.env.MONITORING_TEXTFILE_DIR ?? '.runtime/monitoring/textfile');
const output = resolve(textfileDir, 'forge_agent_health.prom');
const workerCount = Number(process.env.MONITORING_WORKER_COUNT ?? '8');
const timeoutMs = Number(process.env.MONITORING_HTTP_TIMEOUT_MS ?? '5000');

if (!Number.isInteger(workerCount) || workerCount < 1 || workerCount > 32) {
  throw new Error('MONITORING_WORKER_COUNT must be between 1 and 32');
}

const escapeLabel = (value) => String(value).replaceAll('\\', '\\\\').replaceAll('"', '\\"').replaceAll('\n', '\\n');
const metric = (name, labels, value) => {
  const rendered = Object.entries(labels)
    .map(([key, item]) => `${key}="${escapeLabel(item)}"`)
    .join(',');
  return `${name}{${rendered}} ${value}`;
};

const isActive = (scope, unit) => {
  try {
    execFileSync('systemctl', scope === 'user'
      ? ['--user', 'is-active', '--quiet', unit]
      : ['is-active', '--quiet', unit], { stdio: 'ignore' });
    return 1;
  } catch {
    return 0;
  }
};

async function httpCheck(name, url) {
  const started = performance.now();
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const response = await fetch(url, { signal: controller.signal });
    return {
      name,
      ok: response.ok ? 1 : 0,
      status: response.status,
      duration: (performance.now() - started) / 1000,
    };
  } catch {
    return {
      name,
      ok: 0,
      status: 0,
      duration: (performance.now() - started) / 1000,
    };
  } finally {
    clearTimeout(timer);
  }
}

const httpTargets = [
  ['forge_api_ready', process.env.MONITORING_API_READY_URL ?? 'http://127.0.0.1:3100/ready'],
  ['cube_web_health', process.env.MONITORING_CUBE_WEB_URL ?? 'http://127.0.0.1:12088/health'],
  ['cube_api_health', process.env.MONITORING_CUBE_API_URL ?? 'http://127.0.0.1:3001/health'],
  ['cube_ops_health', process.env.MONITORING_CUBE_OPS_URL ?? 'http://127.0.0.1:3010/health'],
  ['alert_sink', process.env.MONITORING_ALERT_SINK_URL ?? 'http://127.0.0.1:29198/health'],
];
const httpResults = await Promise.all(httpTargets.map(([name, url]) => httpCheck(name, url)));
const userUnits = [
  ['forge_api', 'erp-agent-api.service'],
  ...Array.from({ length: workerCount }, (_, index) => [
    `forge_worker_${index + 1}`,
    `erp-agent-worker@${index + 1}.service`,
  ]),
];
const cubeUnits = [
  'cube-sandbox-cube-api.service',
  'cube-sandbox-cubeops.service',
  'cube-sandbox-cubelet.service',
  'cube-sandbox-cube-proxy.service',
  'cube-sandbox-webui.service',
];

const now = Date.now() / 1000;
const lines = [
  '# HELP forge_monitoring_snapshot_timestamp_seconds Unix time of the latest host health snapshot.',
  '# TYPE forge_monitoring_snapshot_timestamp_seconds gauge',
  `forge_monitoring_snapshot_timestamp_seconds ${now}`,
  '# HELP forge_monitoring_check Whether a local HTTP health check succeeded.',
  '# TYPE forge_monitoring_check gauge',
  '# HELP forge_monitoring_check_duration_seconds Duration of a local HTTP health check.',
  '# TYPE forge_monitoring_check_duration_seconds gauge',
  '# HELP forge_monitoring_http_status Last HTTP status observed by a local health check.',
  '# TYPE forge_monitoring_http_status gauge',
  '# HELP forge_agent_service_active Whether a Forge user service is active.',
  '# TYPE forge_agent_service_active gauge',
  '# HELP forge_agent_worker_active Whether a Forge worker service is active.',
  '# TYPE forge_agent_worker_active gauge',
  '# HELP forge_cube_service_active Whether a Cube system service is active.',
  '# TYPE forge_cube_service_active gauge',
];

for (const result of httpResults) {
  lines.push(metric('forge_monitoring_check', { service: result.name }, result.ok));
  lines.push(metric('forge_monitoring_check_duration_seconds', { service: result.name }, result.duration));
  lines.push(metric('forge_monitoring_http_status', { service: result.name }, result.status));
}
for (const [name, unit] of userUnits) {
  const active = isActive('user', unit);
  lines.push(metric(name.startsWith('forge_worker_') ? 'forge_agent_worker_active' : 'forge_agent_service_active', { unit }, active));
}
for (const unit of cubeUnits) {
  lines.push(metric('forge_cube_service_active', { unit }, isActive('system', unit)));
}

await mkdir(textfileDir, { recursive: true, mode: 0o750 });
const temporary = `${output}.tmp-${process.pid}`;
await writeFile(temporary, `${lines.join('\n')}\n`, { mode: 0o640 });
await rename(temporary, output);
