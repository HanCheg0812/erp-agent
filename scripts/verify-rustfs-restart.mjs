import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdir, readFile, writeFile } from 'node:fs/promises';

const origin = process.env.VERIFY_API_ORIGIN ?? 'http://127.0.0.1:3100';
const reportPath = process.env.RUST_LIVE_REPORT ?? '/tmp/erp-rust-live/report.json';
const report = JSON.parse(await readFile(reportPath, 'utf8'));
assert(report.objects?.length, 'The live report has no RustFS objects');

const login = await fetch(`${origin}/v1/auth/login`, {
  method: 'POST',
  headers: { 'content-type': 'application/json' },
  body: JSON.stringify({ email: process.env.DEMO_EMAIL, password: process.env.DEMO_PASSWORD }),
});
if (login.status !== 200) throw new Error(`Login failed (${login.status}): ${await login.text()}`);
const { access_token: token } = await login.json();

const checked = [];
for (const item of report.objects) {
  const response = await fetch(`${origin}/v1/documents/${item.id}`, { headers: { authorization: `Bearer ${token}` } });
  if (response.status !== 200) throw new Error(`${item.filename}: HTTP ${response.status} ${await response.text()}`);
  const bytes = Buffer.from(await response.arrayBuffer());
  const sha256 = createHash('sha256').update(bytes).digest('hex');
  assert.equal(sha256, item.sha256, `${item.filename}: content hash changed`);
  checked.push({ id: item.id, filename: item.filename, bytes: bytes.length, sha256 });
}

const output = { checked_at: new Date().toISOString(), origin, checked };
await mkdir('/tmp/erp-rust-live', { recursive: true });
await writeFile('/tmp/erp-rust-live/rustfs-restart.json', JSON.stringify(output, null, 2));
console.log(`PASS RustFS object persistence: ${checked.length} object(s), hashes unchanged`);
