import { randomBytes } from 'node:crypto';
import { writeFile, readFile, appendFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

const password = randomBytes(24).toString('hex');
const rustfsSecret = randomBytes(32).toString('base64url');
const contents = [
  'POSTGRES_USER=erp_agent',
  `POSTGRES_PASSWORD=${password}`,
  'POSTGRES_DB=erp_agent',
  'POSTGRES_PORT=55432',
  `DATABASE_URL=postgresql://erp_agent:${password}@127.0.0.1:55432/erp_agent`,
  'RUSTFS_ACCESS_KEY=erp_agent_local',
  `RUSTFS_SECRET_KEY=${rustfsSecret}`,
  'RUSTFS_REGION=us-east-1',
  'RUSTFS_S3_PORT=9020',
  'RUSTFS_CONSOLE_PORT=9021',
  'RUSTFS_ENDPOINT=http://127.0.0.1:9020',
  'TEI_PORT=8080',
  'TEI_MODEL_ID=BAAI/bge-m3',
  'TEI_MODEL_REVISION=5617a9f61b028005a4858fdac845db406aefb181',
  'RAG_EMBEDDING_URL=http://127.0.0.1:8080',
  'RAG_EMBEDDING_MODEL_ID=bge-m3',
  'API_HOST=127.0.0.1',
  'API_PORT=3100',
  'DEMO_TENANT=demo',
  'DEMO_EMAIL=planner@example.test',
  `DEMO_PASSWORD=${randomBytes(18).toString('base64url')}`,
  '',
].join('\n');
try {
  await writeFile(new URL('../.env', import.meta.url), contents, { flag: 'wx', mode: 0o600 });
  console.log(`Created ${fileURLToPath(new URL('../.env', import.meta.url))}; credentials were not printed.`);
} catch (error) {
  if (error.code !== 'EEXIST') throw error;
  const path = new URL('../.env', import.meta.url);
  const existing = await readFile(path, 'utf8');
  const names = new Set(existing.split('\n').map(line => line.split('=')[0]));
  const additions = contents.split('\n').filter(line => /^(RUSTFS_|TEI_|RAG_EMBEDDING_)/.test(line) && !names.has(line.split('=')[0]));
  if (additions.length) await appendFile(path, `\n${additions.join('\n')}\n`);
  console.log('Existing credentials preserved; missing service settings added.');
}
