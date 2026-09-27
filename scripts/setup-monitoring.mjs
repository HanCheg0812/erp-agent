import { randomBytes } from 'node:crypto';
import { chmod, mkdir, readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const root = resolve(import.meta.dirname, '..');
const sourcePath = resolve(root, '.env');
const outputDir = resolve(root, '.runtime/monitoring');
const outputPath = resolve(root, '.runtime/monitoring.env');

const parseEnv = (content) => Object.fromEntries(content
  .split(/\r?\n/u)
  .map((line) => line.trim())
  .filter((line) => line && !line.startsWith('#'))
  .map((line) => {
    const separator = line.indexOf('=');
    if (separator < 1) return null;
    const key = line.slice(0, separator).trim();
    let value = line.slice(separator + 1).trim();
    if ((value.startsWith('"') && value.endsWith('"')) || (value.startsWith("'") && value.endsWith("'"))) {
      value = value.slice(1, -1);
    }
    return [key, value];
  })
  .filter(Boolean));

const existing = await readFile(outputPath, 'utf8').catch(() => '');
const source = parseEnv(await readFile(sourcePath, 'utf8'));
const previous = parseEnv(existing);
const value = (key) => process.env[key] ?? source[key] ?? '';
const required = ['POSTGRES_USER', 'POSTGRES_PASSWORD', 'POSTGRES_DB'];
for (const key of required) {
  if (!value(key)) throw new Error(`${key} is required in .env`);
}

const port = value('POSTGRES_PORT') || '55432';
if (!/^\d+$/.test(port)) throw new Error('POSTGRES_PORT must be numeric');

const encoded = (item) => encodeURIComponent(item);
const dataSourceName = `postgresql://${encoded(value('POSTGRES_USER'))}:${encoded(value('POSTGRES_PASSWORD'))}@127.0.0.1:${port}/${encoded(value('POSTGRES_DB'))}?sslmode=disable`;
const grafanaPassword = previous.GRAFANA_ADMIN_PASSWORD || randomBytes(24).toString('base64url');
const lines = [
  `DATA_SOURCE_NAME=${dataSourceName}`,
  'GRAFANA_ADMIN_USER=forge-monitor',
  `GRAFANA_ADMIN_PASSWORD=${grafanaPassword}`,
  'PROMETHEUS_RETENTION=30d',
  'PROMETHEUS_PORT=29190',
  'ALERTMANAGER_PORT=29193',
  'GRAFANA_PORT=13200',
  'MONITORING_WORKER_COUNT=8',
  `MONITORING_HOST_UID=${process.getuid?.() ?? 1000}`,
  `MONITORING_HOST_GID=${process.getgid?.() ?? 1000}`,
].join('\n') + '\n';

await mkdir(outputDir, { recursive: true, mode: 0o750 });
await writeFile(outputPath, lines, { mode: 0o600 });
await chmod(outputPath, 0o600);
console.log(JSON.stringify({
  config: '.runtime/monitoring.env',
  prometheus: 'http://127.0.0.1:29190',
  alertmanager: 'http://127.0.0.1:29193',
  grafana: 'http://127.0.0.1:13200',
  alert_sink: 'http://127.0.0.1:29198/health',
}, null, 2));
