import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const workerCount = Number(process.env.DEV_WORKER_COUNT ?? '8');
const slotsPerWorker = Number(process.env.AGENT_MAX_CONCURRENT_RUNS ?? '8');

if (!Number.isInteger(workerCount) || workerCount < 1 || workerCount > 8) {
  throw new Error('DEV_WORKER_COUNT must be an integer between 1 and 8');
}
if (!Number.isInteger(slotsPerWorker) || slotsPerWorker < 1 || slotsPerWorker > 8) {
  throw new Error('AGENT_MAX_CONCURRENT_RUNS must be an integer between 1 and 8');
}

const run = (command, args, options = {}) => execFileSync(command, args, {
  cwd: root,
  stdio: options.silent ? 'ignore' : 'inherit',
});

// Keep the manager environment aligned with the variable read by the Rust worker.
run('systemctl', ['--user', 'set-environment', 'AGENT_MAX_CONCURRENT_RUNS=' + slotsPerWorker]);

for (let index = 1; index <= workerCount; index += 1) {
  const unit = `erp-agent-dev-worker${workerCount}-${index}.service`;
  const active = (() => {
    try {
      run('systemctl', ['--user', 'is-active', '--quiet', unit], { silent: true });
      return true;
    } catch {
      return false;
    }
  })();

  if (active) {
    run('systemctl', ['--user', 'restart', unit]);
    continue;
  }

  run('systemd-run', [
    '--user',
    '--unit', unit,
    '--working-directory', root,
    '--setenv=AGENT_WORKER_ENABLED=1',
    `--setenv=AGENT_MAX_CONCURRENT_RUNS=${slotsPerWorker}`,
    resolve(root, 'services/api/target/debug/erp-agent-api'),
    'worker',
  ]);
}

console.log(`Started ${workerCount} Rust workers with ${slotsPerWorker} slots each (${workerCount * slotsPerWorker} total).`);
