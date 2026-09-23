import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
const cwd = fileURLToPath(new URL('../external/jcode', import.meta.url));
const git = (...args) => execFileSync('git', args, { cwd, encoding: 'utf8', timeout: 30_000 }).trim();
const pinned = git('rev-parse', 'HEAD');
const remote = git('ls-remote', 'https://github.com/1jehuang/jcode.git', 'HEAD').split(/\s/)[0];
console.log(JSON.stringify({ component: 'jcode', pinned, upstream: remote, updateAvailable: pinned !== remote,
  dirty: git('status', '--porcelain').length > 0 }, null, 2));
// Checking upstream does not update the source or the superproject's pinned commit.
