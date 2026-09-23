import { mkdir, writeFile, rm } from 'node:fs/promises';
import { spawn } from 'node:child_process';
import { randomUUID } from 'node:crypto';

const templateID = 'tpl-erp-agent-base-20260921';
const api = 'http://127.0.0.1:3001';
const existing = await fetch(`${api}/templates/${templateID}`);
if (existing.ok) throw new Error('Target template already exists; inspect its state before retrying');
if (existing.status !== 404) throw new Error(`Template lookup returned ${existing.status}`);
const response = await fetch(`${api}/templates/tpl-ibt20260628090657`);
if (!response.ok) throw new Error(`Source template lookup returned ${response.status}`);
const source = await response.json();
const request = source.createRequest;
request.requestID = randomUUID();
request.annotations['cube.master.appsnapshot.template.id'] = templateID;
request.labels['cube.master.appsnapshot.template.id'] = templateID;
request.labels.project = 'erp-agent';
const dir = new URL('../../.runtime/', import.meta.url);
const file = new URL('cube-template-recovery.json', dir);
await mkdir(dir, { recursive: true, mode: 0o700 });
await writeFile(file, JSON.stringify(request), { mode: 0o600 });
try {
  const child = spawn('/usr/local/services/cubetoolbox/CubeMaster/bin/cubemastercli', [
    '-a', '127.0.0.1', '--timeout', '180s', 'template', 'create',
    '--file', file.pathname, '--node', '192.168.100.122', '--allow-internet-access=false',
  ], { stdio: 'inherit' });
  const exitCode = await new Promise((resolve, reject) => { child.once('exit', resolve); child.once('error', reject); });
  process.exitCode = exitCode ?? 1;
} finally {
  await rm(file, { force: true });
}
