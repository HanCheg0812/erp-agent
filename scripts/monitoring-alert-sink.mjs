import { appendFile, mkdir } from 'node:fs/promises';
import { resolve } from 'node:path';
import { createServer } from 'node:http';

const root = resolve(import.meta.dirname, '..');
const host = process.env.MONITORING_ALERT_SINK_HOST ?? '127.0.0.1';
const port = Number(process.env.MONITORING_ALERT_SINK_PORT ?? '29198');
const logPath = resolve(root, process.env.MONITORING_ALERT_LOG ?? '.runtime/monitoring/alerts.jsonl');
const maxBodyBytes = 1024 * 1024;

if (!Number.isInteger(port) || port < 1024 || port > 65535) {
  throw new Error('MONITORING_ALERT_SINK_PORT must be between 1024 and 65535');
}

await mkdir(resolve(logPath, '..'), { recursive: true, mode: 0o750 });

const readBody = (request) => new Promise((resolveBody, reject) => {
  let total = 0;
  const chunks = [];
  request.on('data', (chunk) => {
    total += chunk.length;
    if (total > maxBodyBytes) {
      reject(new Error('request body too large'));
      request.destroy();
      return;
    }
    chunks.push(chunk);
  });
  request.on('end', () => resolveBody(Buffer.concat(chunks).toString('utf8')));
  request.on('error', reject);
});

const server = createServer(async (request, response) => {
  if (request.method === 'GET' && request.url === '/health') {
    response.writeHead(200, { 'content-type': 'application/json', 'cache-control': 'no-store' });
    response.end('{"status":"ok"}\n');
    return;
  }
  if (request.method !== 'POST' || request.url !== '/alerts') {
    response.writeHead(404);
    response.end();
    return;
  }
  try {
    const payload = JSON.parse(await readBody(request));
    const summary = {
      received_at: new Date().toISOString(),
      status: payload.status ?? 'unknown',
      receiver: payload.receiver ?? 'unknown',
      group_labels: payload.groupLabels ?? {},
      alerts: Array.isArray(payload.alerts)
        ? payload.alerts.map((alert) => ({
          status: alert.status,
          labels: alert.labels,
          annotations: alert.annotations,
          startsAt: alert.startsAt,
          endsAt: alert.endsAt,
        }))
        : [],
    };
    await appendFile(logPath, `${JSON.stringify(summary)}\n`, { mode: 0o640 });
    response.writeHead(204);
    response.end();
  } catch (error) {
    response.writeHead(error instanceof SyntaxError ? 400 : 413, { 'content-type': 'text/plain' });
    response.end(error instanceof SyntaxError ? 'invalid JSON\n' : 'request rejected\n');
  }
});

server.listen(port, host);
const stop = () => server.close(() => process.exit(0));
process.on('SIGTERM', stop);
process.on('SIGINT', stop);
