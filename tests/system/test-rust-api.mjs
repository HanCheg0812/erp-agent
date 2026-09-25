import assert from 'node:assert/strict';
import { randomUUID, randomBytes, scryptSync, createHash, createHmac } from 'node:crypto';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { createServer } from 'node:http';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import pg from 'pg';
import { testBusinessActions } from './test-business-actions.mjs';
import { originalStorageFixture, testOriginalBoundaries, testMemoryTransaction } from './test-layer-boundaries.mjs';

const binary = new URL('../../services/api/target/debug/erp-agent-api', import.meta.url).pathname;
const fixtureDir = await mkdtemp(join(tmpdir(), 'erp-api-fixture-'));
const fixtureBinary = join(fixtureDir, 'runtime');
const quote = value => `'${value.replaceAll("'", "'\\''")}'`;
await writeFile(fixtureBinary, `#!/bin/sh\nexec ${quote(process.execPath)} ${quote(new URL('../../scripts/fixtures/runtime-stub.mjs', import.meta.url).pathname)}\n`, { mode: 0o700 });
const admin = new pg.Client({ connectionString: process.env.DATABASE_URL });
await admin.connect();
const name = `erp_rust_test_${randomBytes(8).toString('hex')}`;
await admin.query(`CREATE DATABASE ${name}`);
const url = new URL(process.env.DATABASE_URL); url.pathname = name;
const db = new pg.Client({ connectionString: url.href }); await db.connect();
let child; let second; let logs = ''; let creates = 0; let erpRequests = 0; const erpAuthHeaders = []; let remote = null; let loseCreate = false; let uncertainDelete = false; let cubeTurnCompleted = false;
const originals = originalStorageFixture();
const cube = createServer(async (req, res) => {
  const chunks = []; for await (const chunk of req) chunks.push(chunk);
  const bytes = Buffer.concat(chunks);
  if (originals.handle(req, res, bytes)) return;
  const input = bytes.toString('utf8');
  res.setHeader('content-type', 'application/json');
  if (req.url === '/responses') {
    const body=JSON.parse(input); assert.equal(body.tools,undefined); assert.equal(body.stream,false);
    const request=JSON.parse(body.input); const text=JSON.stringify(request.arguments);
    return res.end(JSON.stringify({output:[{type:'message',content:[{type:'output_text',text:text.includes('malformed')?'invalid':JSON.stringify({decision:text.includes('deny')?'denied':'allowed',reason:'Synthetic independent review'})}]}]}));
  }
  if (req.url.startsWith('/api/resource/')) {
    erpRequests++;
    erpAuthHeaders.push(req.headers.authorization);
    assert(['token synthetic-erp', 'token forge-reader-token', 'token limited-reader-token'].includes(req.headers.authorization));
    const customer = req.url.includes('OTHER') ? 'OTHER' : 'SYNTHETIC';
    if (req.url.includes('?')) {
      const query = new URL(req.url, 'http://fixture').searchParams;
      const filters = JSON.parse(query.get('filters'))[0];
      assert.deepEqual(filters, ['customer','in',[customer]]);
      if (req.headers.authorization === 'token limited-reader-token' && customer === 'OTHER') {
        res.statusCode = 403;
        return res.end(JSON.stringify({ exc_type: 'PermissionError' }));
      }
      return res.end(JSON.stringify({ data:[{name:'SO-1',customer,modified:'2026-09-24T00:00:00Z'},{name:'SO-OTHER',customer:'OTHER',modified:'2026-09-24T00:00:00Z'}] }));
    }
    if (req.headers.authorization === 'token limited-reader-token' && customer === 'OTHER') {
      res.statusCode = 403;
      return res.end(JSON.stringify({ exc_type: 'PermissionError' }));
    }
    return res.end(JSON.stringify({data:{name:'SO-1',customer,modified:'2026-09-24T00:00:00Z',secret:'must-not-leak',items:[{item_code:'TEST-42',qty:3},{item_code:'OTHER',qty:7}]}}));
  }
  if (req.url === '/api/v1/search/find') {
    assert.deepEqual(JSON.parse(input).target_uri, ['viking://resources/test/user']);
    return res.end(JSON.stringify({result:{resources:[{uri:'viking://resources/test/user/doc',abstract:'Evidence'},{uri:'viking://resources/test/other/doc',abstract:'Private'}]}}));
  }
  if (req.url.startsWith('/api/v1/content/read')) return res.end(JSON.stringify({result:'Authorized source text'}));
  if (req.url === '/templates/test') return res.end(JSON.stringify({ status: 'READY', replicas: [{ status: 'READY', node_ip: '127.0.0.1' }] }));
  if (req.url === '/internal/v1/nodes') return res.end(JSON.stringify([{ IP: '127.0.0.1', Healthy: true }]));
  if (req.url === '/sandboxes' && req.method === 'POST') {
    creates++; const body = JSON.parse(input); remote = { sandboxID: 'test-sandbox', templateID: 'test', state: 'running', metadata: body.metadata };
    if (env.AGENT_EXECUTION_BACKEND === 'cube') {
      assert.equal(body.allow_internet_access, false);
      assert(Array.isArray(body.network?.allow_out));
      assert(body.network.allow_out.includes('api.deepseek.com'));
      assert.equal(body.cube_network_config, undefined);
      assert.equal(body.envVars, undefined);
    }
    if (loseCreate) return req.socket.destroy();
    return res.end(JSON.stringify(remote));
  }
  if (req.url.startsWith('/sandboxes?')) return res.end(JSON.stringify(remote ? [remote] : []));
  if (req.url === '/sandboxes/test-sandbox') {
    if (req.method === 'DELETE') { if (!uncertainDelete) remote = null; res.statusCode = 204; return res.end(); }
    if (!remote) res.statusCode = 404;
    return res.end(JSON.stringify(remote));
  }
  if (req.url === '/sandbox/test-sandbox/49999/health/ready') return res.end(JSON.stringify({ status: 'ready', mode: 'resident', warmup: false }));
  if (req.url === '/sandbox/test-sandbox/49999/v1/configure') {
    assert.equal(req.method, 'POST');
    const config = JSON.parse(input).env;
    assert.equal(config.CUBE_TEMPLATE_WARMUP, '0');
    assert.equal(config.AGENT_RUNTIME_MODE, 'resident');
    assert(config.JCODE_MCP_SESSION_ID.startsWith('mcp_'));
    return res.end(JSON.stringify({ status: 'ready' }));
  }
  if (req.url === '/sandbox/test-sandbox/49999/v1/turn') {
    const request = JSON.parse(input);
    const expected = createHmac('sha256', env.CUBE_RUNTIME_SHARED_SECRET).update('erp-agent/cube-runtime/v1/').update(Buffer.from(remote.metadata.erp_operation.replaceAll('-', ''), 'hex')).digest('hex');
    assert.equal(req.headers['x-agent-runtime-token'], expected);
    assert.equal(request.case.id, remote.metadata.erp_case);
    assert.equal(request.new_session, true);
    res.setHeader('content-type', 'application/x-ndjson');
    const source = 'Synthetic test guidance';
    const snapshot = JSON.stringify({ schema_version: 1, id: 'test@1', skills: [{ id: 'test-review', version: '1.0.0', source, sha256: createHash('sha256').update(source).digest('hex') }] });
    res.write(`${JSON.stringify({ type: 'skill_catalog', snapshot, hash: createHash('sha256').update(snapshot).digest('hex') })}\n`);
    await new Promise(resolve => setTimeout(resolve, 150));
    res.write(`${JSON.stringify({ type: 'text_delta', text: 'Streamed from Cube' })}\n`);
    await new Promise(resolve => setTimeout(resolve, 650));
    res.end(`${JSON.stringify({ type: 'completed' })}\n`);
    cubeTurnCompleted = true;
    return;
  }
  if (req.url.endsWith('/health')) return res.end(JSON.stringify('OK'));
  res.statusCode = 404; res.end('{}');
});
cube.listen(0, '127.0.0.1'); await once(cube, 'listening');
const listener = createServer(); listener.listen(0, '127.0.0.1'); await once(listener, 'listening'); const port = listener.address().port; await new Promise(resolve => listener.close(resolve));
const origin = `http://127.0.0.1:${port}`;
const env = { ...process.env, ERP_AGENT_LOAD_CUBE_ENV:'0', API_INTERNAL_HOST:'', DATABASE_URL: url.href, API_PORT: String(port), DEEPSEEK_API_KEY: '', ERP_API_TOKEN:'synthetic-erp', ERP_CREDENTIAL_FORGE_READER:'forge-reader-token', ERP_CREDENTIAL_LIMITED_READER:'limited-reader-token', ERP_API_URL:`http://127.0.0.1:${cube.address().port}`, ERP_SHARED_DEMO_MODE:'true', OPENVIKING_URL:`http://127.0.0.1:${cube.address().port}`, OPENVIKING_API_KEY:'', CUBE_API_URL: `http://127.0.0.1:${cube.address().port}`, CUBE_OPS_URL: `http://127.0.0.1:${cube.address().port}`, CUBE_PROXY_URL: `http://127.0.0.1:${cube.address().port}`, CUBE_TEMPLATE_ID: 'test' };
env.AGENT_MAX_CONCURRENT_RUNS = '4';
env.RUSTFS_ENDPOINT=`http://127.0.0.1:${cube.address().port}`;
env.RUSTFS_BUCKET='fixture-originals'; env.RUSTFS_ACCESS_KEY='synthetic'; env.RUSTFS_SECRET_KEY='synthetic-test-only';
env.AGENT_STORAGE_ORIGIN=origin;
env.AGENT_REVIEWER_BASE_URL=`http://127.0.0.1:${cube.address().port}`;
async function command(args) { const p = spawn(binary, args, { env, stdio: ['ignore', 'ignore', 'pipe'] }); let errors = ''; p.stderr.on('data', c => { errors += c; }); assert.equal((await once(p, 'exit'))[0], 0, errors); }
async function start() {
  child = spawn(binary, [], { env, stdio: ['ignore', 'ignore', 'pipe'] }); child.stderr.on('data', c => { logs += c; });
  for (let i = 0; i < 100; i++) { if (await fetch(`${origin}/ready`).then(r => r.ok).catch(() => false)) return; await new Promise(resolve => setTimeout(resolve, 50)); }
  throw new Error(`API did not start: ${logs}`);
}
async function stop() { if (child && child.exitCode === null) { child.kill('SIGTERM'); await once(child, 'exit'); } }
async function api(path, { token, method = 'GET', body, status = 200, headers = {}, raw = false } = {}) {
  const r = await fetch(`${origin}${path.startsWith('/internal/') ? '' : '/v1'}${path}`, { method, headers: { ...(body === undefined ? {} : { 'content-type': 'application/json' }), ...(token ? { authorization: `Bearer ${token}` } : {}), ...headers }, ...(body === undefined ? {} : { body: JSON.stringify(body) }) });
  const data = raw ? Buffer.from(await r.arrayBuffer()) : r.status === 204 ? null : await r.json();
  assert([status].flat().includes(r.status), `${method} ${path}: status ${r.status}, ${JSON.stringify(data)}\n${logs}`); return data;
}
const pass = `test-${randomBytes(18).toString('hex')}`;
const hash = password => { const salt = randomBytes(16).toString('hex'); return `scrypt:${salt}:${scryptSync(password, salt, 64).toString('hex')}`; };
const note = message => console.log(`PASS ${message}`);
try {
  await command(['migrate']); await command(['migrate']); await start();
  const checksum = (await db.query("SELECT checksum FROM schema_migrations WHERE name='20260922105658_reissue_runtime_leases'")).rows[0].checksum;
  await db.query("UPDATE schema_migrations SET checksum='changed' WHERE name='20260922105658_reissue_runtime_leases'");
  assert.equal((await fetch(`${origin}/ready`)).status, 503);
  await db.query("UPDATE schema_migrations SET checksum=$1 WHERE name='20260922105658_reissue_runtime_leases'", [checksum]);
  assert.equal((await fetch(`${origin}/ready`)).status, 200);
  await api('/cases', { status: 401 });
  await api('/auth/register', { method: 'POST', body: { email: 'bad', password: pass }, status: 400 });
  const a = (await api('/auth/register', { method: 'POST', body: { email: 'person-a@example.test', password: pass }, status: 201 })).access_token;
  const b = (await api('/auth/register', { method: 'POST', body: { email: 'person-b@example.test', password: pass }, status: 201 })).access_token;
  const actor = await api('/auth/me', { token: a });
  await api('/auth/login', { method: 'POST', body: { email: 'person-a@example.test', password: 'wrong-password' }, status: 401 });
  await api('/auth/login', { method: 'POST', body: { tenant: 'not-this-tenant', email: 'person-a@example.test', password: pass }, status: 401 });
  const userHash = (await db.query('SELECT password_hash FROM users WHERE id=$1', [actor.userId])).rows[0].password_hash;
  assert(userHash.startsWith('$argon2id$')); assert.deepEqual(actor.roles, ['planner']);
  await api('/auth/register', { method: 'POST', body: { email: 'PERSON-A@EXAMPLE.TEST', password: pass }, status: 409 });
  const cId = randomUUID(); await db.query("INSERT INTO users(id,tenant_id,email,password_hash,roles) VALUES($1,$2,'same-tenant@example.test',$3,ARRAY['planner','approver'])", [cId, actor.tenantId, hash(pass)]);
  const c = (await api('/auth/login', { method: 'POST', body: { email: 'same-tenant@example.test', password: pass } })).access_token;
  assert((await db.query('SELECT password_hash FROM users WHERE id=$1', [cId])).rows[0].password_hash.startsWith('$argon2id$'));
  note('email registration, case normalization, Argon2id, legacy scrypt upgrade');
  await Promise.all([0, 1].map(() => api('/auth/register', { method: 'POST', body: { email: 'concurrent@example.test', password: pass }, status: [201, 409] })));
  assert.equal((await db.query("SELECT count(*)::int AS n FROM users WHERE email='concurrent@example.test'")).rows[0].n, 1);
  const create = { title: 'Synthetic Rust acceptance', customer_code: 'SYNTHETIC', part_number: 'TEST-42', revision: 'C', demand_type: 'order', quantity_basis: 'replacement' };
  const record = await api('/cases', { token: a, method: 'POST', body: create, status: 201 }); const id = record.id;
  const evidenceId = randomUUID();
  await db.query("INSERT INTO documents(id,tenant_id,owner_id,case_id,object_key,filename,content_type,bytes,sha256,state) VALUES($1,$2,$3,$4,$5,'delivery-note.txt','text/plain',12,$6,'ready')", [evidenceId, actor.tenantId, actor.userId, id, `tests/${evidenceId}`, 'a'.repeat(64)]);
  for (const token of [b, c]) {
    assert.equal((await api('/cases', { token })).items.length, 0);
    for (const suffix of ['', '/events', '/conversation', '/documents', '/sandbox']) await api(`/cases/${id}${suffix}`, { token, status: 404 });
    await api(`/cases/${id}/agent-sessions`, { token, method: 'POST', body: {}, status: 404 });
    await api(`/cases/${id}/conversation/messages`, { token, method: 'POST', body: { client_message_id: randomUUID(), text: 'unauthorized' }, status: 404 });
  }
  const proposal = { expected_case_version: 1, summary: 'Replace total, retain revision C.', actions: [{ type: 'revise_sales_order_item', sales_order: 'SYNTHETIC-1', item_row: '10', expected_erp_modified: '2026-09-22T00:00:00Z', expected_quantity: 960, proposed_quantity: 1280, expected_delivery_date: '2026-10-28', proposed_delivery_date: '2026-10-22' }], evidence: [{ document_id: evidenceId, revision: 'C', locator: 'page 2 table 1' }] };
  const newRequest = {request_id:randomUUID()};
  const plain = await api('/conversations', {token:a, method:'POST', body:newRequest, status:201});
  assert.equal(plain.kind, 'conversation'); assert.equal(plain.customer_code, null);
  assert.equal((await api('/conversations', {token:a, method:'POST', body:newRequest})).id, plain.id);
  await api(`/cases/${plain.id}/proposals`, {token:a, method:'POST', body:proposal, status:422});
  await api(`/cases/${plain.id}/conversation/messages`, {token:a, method:'POST', body:{client_message_id:randomUUID(),text:'Start directly'}, status:201});
  const plainHistory = await api(`/cases/${plain.id}/conversation`, {token:a});
  const savedSettings = { approval: 'auto', model: 'deepseek-flash', effort: 'low' };
  assert.deepEqual((await api(`/cases/${plain.id}/conversation/settings`, { token: a, method: 'PATCH', body: savedSettings })).settings, savedSettings);
  assert.deepEqual((await api(`/cases/${plain.id}/conversation`, { token: a })).settings, savedSettings);
  await api(`/cases/${plain.id}/conversation/settings`, { token: b, method: 'PATCH', body: savedSettings, status: 404 });
  await api(`/cases/${plain.id}/conversation/settings`, { token: a, method: 'PATCH', body: { ...savedSettings, approval: 'invalid' }, status: 400 });
  assert.equal((await api(`/cases/${plain.id}`, {token:a})).title, 'Start directly');
  const context = {expected_case_version:1,context:create};
  await api(`/cases/${plain.id}/context`, {token:c,method:'PATCH',body:context,status:404});
  const converted = await api(`/cases/${plain.id}/context`, {token:a,method:'PATCH',body:context});
  assert.equal(converted.id, plain.id); assert.equal(converted.version,2);
  assert.equal((await api(`/cases/${plain.id}/context`, {token:a,method:'PATCH',body:context})).id, plain.id);
  assert.equal((await api(`/cases/${plain.id}/conversation`, {token:a})).session_id, plainHistory.session_id);
  note('new conversation without case form; idempotent creation, title, optional case context retains session');
  const p = await api(`/cases/${id}/proposals`, { token: a, method: 'POST', body: { ...proposal, summary: `  ${proposal.summary}  ` }, status: 201 });
  assert.equal(p.summary, proposal.summary);
  await api(`/cases/${id}/proposals`, { token: a, method: 'POST', body: proposal, status: 409 });
  const decision = { proposal_id: p.id, proposal_digest: p.digest, expected_case_version: 2, decision: 'approved', comment: 'Synthetic acceptance only' };
  await api(`/cases/${id}/approvals`, { token: a, method: 'POST', body: decision, headers: { 'idempotency-key': randomUUID() }, status: 403 });
  await db.query("UPDATE users SET roles=ARRAY['planner','approver'] WHERE id=$1", [actor.userId]);
  const key = randomUUID(); const options = { token: a, method: 'POST', body: decision, headers: { 'idempotency-key': key }, status: 201 };
  await api(`/cases/${id}/approvals`, options);
  assert.equal((await api(`/cases/${id}/approvals`, { ...options, status: 200 })).replayed, true);
  await api(`/cases/${id}/approvals`, { ...options, body: { ...decision, comment: 'changed' }, status: 409 });
  await api(`/cases/${id}/approvals`, { ...options, token: c, status: 404 });
  assert.equal((await api(`/cases/${id}/events`, { token: a })).items.filter(x => x.event_type === 'proposal.approved').length, 1);
  note('cross-tenant AND same-tenant owner isolation; exact-version approval and duplicate protection');
  const concurrent = await api('/cases', { token: a, method: 'POST', body: create, status: 201 });
  const concurrentEvidence = randomUUID();
  await db.query("INSERT INTO documents(id,tenant_id,owner_id,case_id,object_key,filename,content_type,bytes,sha256,state) VALUES($1,$2,$3,$4,$5,'delivery-note.txt','text/plain',12,$6,'ready')", [concurrentEvidence, actor.tenantId, actor.userId, concurrent.id, `tests/${concurrentEvidence}`, 'b'.repeat(64)]);
  const concurrentProposal = { ...proposal, evidence: [{ ...proposal.evidence[0], document_id: concurrentEvidence }] };
  await Promise.all([0, 1].map(() => api(`/cases/${concurrent.id}/proposals`, { token: a, method: 'POST', body: concurrentProposal, status: [201, 409] })));
  const cp = (await api(`/cases/${concurrent.id}`, { token: a })).proposals;
  assert.equal(cp.length, 1);
  const approveOptions = { ...options, headers: { 'idempotency-key': randomUUID() }, body: { ...decision, proposal_id: cp[0].id, proposal_digest: cp[0].digest }, status: [200, 201] };
  await Promise.all([0, 1].map(() => api(`/cases/${concurrent.id}/approvals`, approveOptions)));
  assert.equal((await api(`/cases/${concurrent.id}/events`, { token: a })).items.filter(x => x.event_type === 'proposal.approved').length, 1);
  const rollbackConversation = await api('/conversations', { token: a, method: 'POST', body: { request_id: randomUUID() }, status: 201 });
  const rollbackCase = await api('/cases', { token: a, method: 'POST', body: create, status: 201 });
  const rollbackEvidence = randomUUID();
  await db.query("INSERT INTO documents(id,tenant_id,owner_id,case_id,object_key,filename,content_type,bytes,sha256,state) VALUES($1,$2,$3,$4,$5,'rollback.txt','text/plain',12,$6,'ready')", [rollbackEvidence, actor.tenantId, actor.userId, rollbackCase.id, `tests/${rollbackEvidence}`, 'c'.repeat(64)]);
  const rollbackProposalInput = { ...proposal, evidence: [{ ...proposal.evidence[0], document_id: rollbackEvidence }] };
  const rollbackProposal = await api(`/cases/${rollbackCase.id}/proposals`, { token: a, method: 'POST', body: rollbackProposalInput, status: 201 });
  const beforeAudit = (await db.query('SELECT count(*)::int AS n FROM cases')).rows[0].n;
  await db.query("CREATE FUNCTION reject_test_audit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'synthetic audit failure'; END $$");
  await db.query('CREATE TRIGGER reject_test_audit BEFORE INSERT ON audit_events FOR EACH ROW EXECUTE FUNCTION reject_test_audit()');
  await api('/cases', { token: a, method: 'POST', body: create, status: 500 });
  assert.equal((await db.query('SELECT count(*)::int AS n FROM cases')).rows[0].n, beforeAudit);
  await api(`/cases/${id}/conversation/messages`, { token: a, method: 'POST', body: { client_message_id: randomUUID(), text: 'Must roll back' }, status: 500 });
  assert.equal((await db.query('SELECT count(*)::int AS n FROM case_conversation_messages WHERE case_id=$1', [id])).rows[0].n, 0);
  await api(`/cases/${rollbackConversation.id}/context`, { token: a, method: 'PATCH', body: context, status: 500 });
  const untouchedContext = await api(`/cases/${rollbackConversation.id}`, { token: a });
  assert.equal(untouchedContext.kind, 'conversation'); assert.equal(untouchedContext.version, 1);
  await api(`/cases/${rollbackCase.id}/proposals`, { token: a, method: 'POST', body: { ...rollbackProposalInput, expected_case_version: 2 }, status: 500 });
  const untouchedProposal = await api(`/cases/${rollbackCase.id}`, { token: a });
  assert.equal(untouchedProposal.version, 2); assert.equal(untouchedProposal.proposals.length, 1);
  const rollbackApprovalKey = randomUUID();
  const rollbackDecision = { ...decision, proposal_id: rollbackProposal.id, proposal_digest: rollbackProposal.digest };
  await api(`/cases/${rollbackCase.id}/approvals`, { token: a, method: 'POST', body: rollbackDecision, headers: { 'idempotency-key': rollbackApprovalKey }, status: 500 });
  assert.equal((await api(`/cases/${rollbackCase.id}`, { token: a })).status, 'awaiting_approval');
  assert.equal((await db.query('SELECT count(*)::int AS n FROM approvals WHERE case_id=$1', [rollbackCase.id])).rows[0].n, 0);
  await db.query('DROP TRIGGER reject_test_audit ON audit_events'); await db.query('DROP FUNCTION reject_test_audit()');
  await api(`/cases/${rollbackCase.id}/approvals`, { token: a, method: 'POST', body: rollbackDecision, headers: { 'idempotency-key': rollbackApprovalKey }, status: 201 });
  note('concurrent registration, proposal and approval; audit failure rolls back context, proposals, approvals and conversations; failed approval key remains retryable');
  const messageInput = { client_message_id: randomUUID(), text: 'Persist my request' };
  const message = await api(`/cases/${id}/conversation/messages`, { token: a, method: 'POST', body: messageInput, status: 201 });
  await db.query("INSERT INTO agent_runs(message_id,tenant_id,case_id,state) VALUES($1,$2,$3,'running')", [message.message.id,actor.tenantId,id]);
  await api(`/cases/${id}/conversation/settings`, { token: a, method: 'PATCH', body: savedSettings, status: 409 });
  await db.query('DELETE FROM agent_runs WHERE message_id=$1',[message.message.id]);
  assert.equal((await api(`/cases/${id}/conversation/messages`, { token: a, method: 'POST', body: messageInput })).message.id, message.message.id);
  await api(`/cases/${id}/conversation/messages`, { token: a, method: 'POST', body: { ...messageInput, text: 'changed' }, status: 409 });
  const history = await api(`/cases/${id}/conversation`, { token: a }); const sid = history.session_id;
  const claim = await api(`/agent-sessions/${sid}/claim`, { token: a, method: 'POST', body: {} });
  await api(`/agent-sessions/${sid}/claim`, { token: c, method: 'POST', body: {}, status: 404 });
  const delta = { request_id: randomUUID(), expected_revision: 0, metadata: { id: sid, title: 'Fixture' }, vectors: Object.fromEntries(['messages', 'env_snapshots', 'memory_injections', 'replay_events'].map(k => [k, { mode: 'append', values: k === 'messages' ? [{ role: 'user', text: 'keep revision C' }] : [] }])) };
  await api(`/internal/agent-sessions/${sid}/deltas`, { token: claim.storage_token, method: 'POST', body: { ...delta, metadata: { id: 'other' } }, status: 400 });
  await api(`/internal/agent-sessions/${sid}/deltas`, { token: claim.storage_token, method: 'POST', body: { ...delta, vectors: { ...delta.vectors, messages: { mode: 'merge', values: [] } } }, status: 400 });
  await api(`/internal/agent-sessions/${sid}/deltas`, { token: claim.storage_token, method: 'POST', body: delta });
  assert.equal((await api(`/internal/agent-sessions/${sid}/deltas`, { token: claim.storage_token, method: 'POST', body: delta })).revision, 1);
  await api(`/internal/agent-sessions/${sid}/deltas`, { token: claim.storage_token, method: 'POST', body: { ...delta, request_id: randomUUID() }, status: 409 });
  const todo = { request_id: randomUUID(), expected_revision: 0, payload: [{ id: 'review', status: 'pending' }] };
  await api(`/internal/agent-sessions/${sid}/documents/arbitrary`, { token: claim.storage_token, status: 400 });
  await api(`/internal/agent-sessions/${sid}/documents/todos`, { token: claim.storage_token, method: 'PUT', body: { ...todo, expected_revision: -1 }, status: 400 });
  await api(`/internal/agent-sessions/${sid}/documents/todos`, { token: claim.storage_token, method: 'PUT', body: todo });
  assert.equal((await api(`/internal/agent-sessions/${sid}/documents/todos`, { token: claim.storage_token, method: 'PUT', body: todo })).revision, 1);
  await api(`/internal/agent-sessions/${sid}/documents/todos`, { token: claim.storage_token, method: 'PUT', body: { ...todo, payload: [] }, status: 409 });
  await api(`/internal/agent-sessions/${sid}/documents/todos`, { token: claim.storage_token, method: 'PUT', body: { request_id: randomUUID(), expected_revision: 0, payload: [] }, status: 409 });
  const newClaim = await api(`/agent-sessions/${sid}/claim`, { token: a, method: 'POST', body: {} });
  const toolPath = `/internal/agent-sessions/${sid}/tools`;
  const toolCall = {domain:'erp',tool:'list_erp_orders',arguments:{}};
  await api(toolPath, {token:a,method:'POST',body:toolCall,status:401});
  await api(toolPath, {token:newClaim.storage_token,method:'POST',body:toolCall,status:403});
  await db.query("INSERT INTO agent_runs(message_id,tenant_id,case_id,state) VALUES($1,$2,$3,'running')", [message.message.id,actor.tenantId,id]);
  await api(toolPath, {token:newClaim.storage_token,method:'POST',body:toolCall,status:403});
  for (const [domain,scope] of [['erp','SYNTHETIC'],['openviking','viking://resources/test/user']]) await db.query('INSERT INTO integration_grants(tenant_id,owner_id,domain,scope) VALUES($1,$2,$3,$4)',[actor.tenantId,actor.userId,domain,scope]);
  await db.query("INSERT INTO integration_grants(tenant_id,owner_id,domain,scope,tool_id,action,effect,resource_type) VALUES($1,$2,'erp','SYNTHETIC','list_erp_orders','read','deny','customer')",[actor.tenantId,actor.userId]);
  await api(toolPath,{token:newClaim.storage_token,method:'POST',body:toolCall,status:403});
  await db.query("DELETE FROM integration_grants WHERE tenant_id=$1 AND owner_id=$2 AND domain='erp' AND scope='SYNTHETIC' AND tool_id='list_erp_orders' AND effect='deny'",[actor.tenantId,actor.userId]);
  const beforeUnboundErpCalls = erpRequests;
  assert.equal((await api(toolPath,{token:newClaim.storage_token,method:'POST',body:toolCall,status:503})).error,'erp_identity_unbound');
  assert.equal(erpRequests,beforeUnboundErpCalls);
  const erpConnection = randomUUID();
  await db.query("INSERT INTO erp_connections(id,tenant_id,name,base_url,auth_mode) VALUES($1,$2,'fixture-erp',$3,'shared_demo')",[erpConnection,actor.tenantId,`http://127.0.0.1:${cube.address().port}`]);
  await db.query("INSERT INTO erp_identity_bindings(tenant_id,platform_user_id,connection_id,erp_user_id,credential_ref) VALUES($1,$2,$3,'forge-reader@example.test','env:ERP_API_TOKEN')",[actor.tenantId,actor.userId,erpConnection]);
  const beforeRejectedErpCalls = erpRequests;
  await db.query('UPDATE erp_identity_bindings SET active=false WHERE tenant_id=$1 AND platform_user_id=$2',[actor.tenantId,actor.userId]);
  assert.equal((await api(toolPath,{token:newClaim.storage_token,method:'POST',body:toolCall,status:503})).error,'erp_identity_inactive');
  await db.query('UPDATE erp_identity_bindings SET active=true WHERE tenant_id=$1 AND platform_user_id=$2',[actor.tenantId,actor.userId]);
  await db.query("UPDATE erp_connections SET auth_mode='bound_user' WHERE id=$1",[erpConnection]);
  await db.query("UPDATE erp_identity_bindings SET credential_ref='env:ERP_CREDENTIAL_FORGE_READER' WHERE tenant_id=$1 AND platform_user_id=$2",[actor.tenantId,actor.userId]);
  const listedOrders = await api(toolPath,{token:newClaim.storage_token,method:'POST',body:toolCall});
  assert.equal(listedOrders.status,'ok');
  assert.equal(listedOrders.data.orders.length,1);
  assert.equal(listedOrders.sources[0].doctype,'Sales Order');
  assert(listedOrders.trace_id);
  assert(erpRequests > beforeRejectedErpCalls);
  assert(erpAuthHeaders.includes('token forge-reader-token'));
  const standaloneMcp = await api('/mcp-sessions', { token: a, method: 'POST', body: {}, status: 201 });
  assert.equal(standaloneMcp.case_id, null);
  const standalonePath = `/internal/mcp-sessions/${standaloneMcp.id}/tools`;
  await api(standalonePath, { token: a, method: 'POST', body: toolCall, status: 401 });
  const standaloneListed = await api(standalonePath, { token: standaloneMcp.capability, method: 'POST', body: toolCall });
  assert.equal(standaloneListed.data.orders.length, 1);
  assert.equal(standaloneListed.data.orders[0].customer, 'SYNTHETIC');
  await api(`/mcp-sessions/${standaloneMcp.id}`, { token: a, method: 'DELETE' });
  await api(standalonePath, { token: standaloneMcp.capability, method: 'POST', body: toolCall, status: 401 });
  const secondCase = await api('/cases', { token: c, method: 'POST', body: { ...create, title: 'Limited ERP identity', customer_code: 'OTHER' }, status: 201 });
  const secondMessage = await api(`/cases/${secondCase.id}/conversation/messages`, { token: c, method: 'POST', body: { client_message_id: randomUUID(), text: 'Read ERP with limited identity' }, status: 201 });
  const secondHistory = await api(`/cases/${secondCase.id}/conversation`, { token: c });
  const secondClaim = await api(`/agent-sessions/${secondHistory.session_id}/claim`, { token: c, method: 'POST', body: {} });
  await db.query("INSERT INTO agent_runs(message_id,tenant_id,case_id,state) VALUES($1,$2,$3,'running')", [secondMessage.message.id,actor.tenantId,secondCase.id]);
  await db.query("INSERT INTO integration_grants(tenant_id,owner_id,domain,scope,tool_id,action,resource_type) VALUES($1,$2,'erp','OTHER','list_erp_orders','read','customer')",[actor.tenantId,cId]);
  await db.query("INSERT INTO erp_identity_bindings(tenant_id,platform_user_id,connection_id,erp_user_id,credential_ref) VALUES($1,$2,$3,'limited-reader@example.test','env:ERP_CREDENTIAL_LIMITED_READER')",[actor.tenantId,cId,erpConnection]);
  const beforeNativeReject = erpRequests;
  const nativeReject = await api(`/internal/agent-sessions/${secondHistory.session_id}/tools`,{token:secondClaim.storage_token,method:'POST',body:toolCall,status:403});
  assert.equal(nativeReject.error,'erp_forbidden');
  assert.equal(nativeReject.agent_guidance.category,'authorization');
  assert.equal(nativeReject.agent_guidance.retryable,false);
  assert.equal(nativeReject.agent_guidance.safe_next_step,'ask_erp_admin');
  assert(nativeReject.agent_guidance.instruction.includes('Do not retry with altered arguments'));
  assert(erpRequests > beforeNativeReject);
  await db.query("UPDATE erp_connections SET auth_mode='shared_demo' WHERE id=$1",[erpConnection]);
  await db.query("UPDATE erp_identity_bindings SET credential_ref='env:ERP_API_TOKEN' WHERE tenant_id=$1 AND platform_user_id=$2",[actor.tenantId,actor.userId]);
  await api(toolPath, {token:newClaim.storage_token,method:'POST',body:{domain:'erp',tool:'unknown',arguments:{}},status:400});
  const listedAgain = await api(toolPath,{token:newClaim.storage_token,method:'POST',body:toolCall});
  assert.equal(listedAgain.data.orders.length,1);
  assert.equal((await db.query("SELECT payload->>'erp_user' AS erp_user,payload->>'trace_id' AS trace_id FROM audit_events WHERE case_id=$1 AND event_type='agent.tool_read' ORDER BY created_at DESC LIMIT 1",[id])).rows[0].erp_user,'forge-reader@example.test');
  assert.equal((await db.query("SELECT payload->>'trace_id' AS trace_id FROM audit_events WHERE case_id=$1 AND event_type='agent.tool_read' ORDER BY created_at DESC LIMIT 1",[id])).rows[0].trace_id,listedAgain.trace_id);
  for (const limit of [0, 21]) await api(toolPath,{token:newClaim.storage_token,method:'POST',body:{...toolCall,arguments:{limit}},status:400});
  const order = await api(toolPath,{token:newClaim.storage_token,method:'POST',body:{domain:'erp',tool:'read_erp_order',arguments:{sales_order:'SO-1'}}});
  assert.equal(order.status,'ok'); assert.equal(order.data.order.items.length,1); assert.equal(order.data.order.secret,undefined); assert.equal(order.sources[0].doctype,'Sales Order');
  await api(toolPath,{token:newClaim.storage_token,method:'POST',body:{domain:'erp',tool:'read_erp_order',arguments:{sales_order:'SO-OTHER'}},status:404});
  const knowledgeCall = {domain:'openviking',tool:'search_case_knowledge',arguments:{query:'Evidence'}};
  assert.equal((await api(toolPath,{token:newClaim.storage_token,method:'POST',body:knowledgeCall})).resources.length,1);
  await api(toolPath,{token:newClaim.storage_token,method:'POST',body:{...knowledgeCall,arguments:{query:'',limit:5}},status:400});
  await api(toolPath,{token:newClaim.storage_token,method:'POST',body:{...knowledgeCall,arguments:{query:'Evidence',limit:11}},status:400});
  for (const uri of ['viking://resources/test/other/doc','viking://resources/test/user/../other','viking://resources/test/user/%2e%2e/other']) await api(toolPath,{token:newClaim.storage_token,method:'POST',body:{domain:'openviking',tool:'read_knowledge_source',arguments:{uri}},status:403});
  assert.equal((await api(toolPath,{token:newClaim.storage_token,method:'POST',body:{domain:'openviking',tool:'read_knowledge_source',arguments:{uri:'viking://resources/test/user/doc'}}})).content,'Authorized source text');
  for (const mode of ['ask','auto','full']) {
    await db.query("UPDATE agent_runs SET settings=jsonb_set(settings,'{approval}',$2::jsonb) WHERE message_id=$1",[message.message.id,JSON.stringify(mode)]);
    assert.equal((await api(toolPath,{token:newClaim.storage_token,method:'POST',body:toolCall})).data.orders.length,1);
  }
  await db.query('DELETE FROM agent_runs WHERE message_id=$1',[message.message.id]);
  await api(`/cases/${id}/conversation/messages`,{token:a,method:'POST',body:{...messageInput,settings:{approval:'ask',model:'deepseek-flash',effort:'low'}},status:409});
  await api(`/cases/${id}/conversation/messages`,{token:a,method:'POST',body:{client_message_id:randomUUID(),text:'invalid',settings:{approval:'invalid',model:'deepseek-flash',effort:'high'}},status:400});
  note('MCP scoped and standalone capabilities, active run, explicit grants, ERP customer/part fields, knowledge path isolation, reads available in all approval modes');
  await api(`/internal/agent-sessions/${sid}`, { token: claim.storage_token, status: 401 });
  const uploaded = await testOriginalBoundaries({ api, db, actor, token: a, otherTokens: [b,c], caseId: id, control: originals.control });
  note('original upload retries, failure recovery, hash verification and owner isolation with controlled S3 fixture');
  const readPath = `/internal/agent-sessions/${sid}/attachments/read`;
  const longText = '中文 segment\n'.repeat(2500);
  const longDocument = await api(`/cases/${id}/documents`, { token: a, method: 'POST', status: 201, body: { request_id: randomUUID(), filename: 'long.md', content_type: 'text/markdown', data_base64: Buffer.from(longText).toString('base64') } });
  const firstPart = await api(readPath, { token: newClaim.storage_token, method: 'POST', body: { document_id: longDocument.id } });
  assert.equal(firstPart.complete, false);
  assert.equal(firstPart.next_offset, 20000);
  const secondPart = await api(readPath, { token: newClaim.storage_token, method: 'POST', body: { document_id: longDocument.id, offset: firstPart.next_offset } });
  assert.equal(secondPart.complete, true);
  assert.equal(firstPart.text + secondPart.text, longText);
  const foreignDocument = await api(`/cases/${plain.id}/documents`, { token: a, method: 'POST', status: 201, body: { request_id: randomUUID(), filename: 'other.txt', content_type: 'text/plain', data_base64: Buffer.from('Other case').toString('base64') } });
  await api(readPath, { token: newClaim.storage_token, method: 'POST', body: { document_id: foreignDocument.id }, status: 404 });
  await api(readPath, { token: a, method: 'POST', body: { document_id: longDocument.id }, status: 401 });
  assert.equal((await db.query("SELECT count(*)::int AS n FROM audit_events WHERE event_type='agent.attachment_read' AND case_id=$1", [id])).rows[0].n, 2);
  note('scoped attachment read continues long text, audits reads, and rejects other cases and browser tokens');
  const imageBytes = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/lS8AAAAASUVORK5CYII=', 'base64');
  const image = await api(`/cases/${id}/documents`, { token: a, method: 'POST', status: 201, body: { request_id: randomUUID(), filename: 'photo.png', content_type: 'image/png', data_base64: imageBytes.toString('base64') } });
  const attachmentInput = { client_message_id: randomUUID(), text: '', attachment_ids: [image.id, uploaded.id] };
  const attached = await api(`/cases/${id}/conversation/messages`, { token: a, method: 'POST', body: attachmentInput, status: 201 });
  assert.deepEqual(attached.message.attachments.map(item => item.id), attachmentInput.attachment_ids);
  assert.equal(attached.message.attachments[0].object_key, undefined);
  assert.deepEqual((await api(`/cases/${id}/conversation/messages`, { token: a, method: 'POST', body: attachmentInput })).message.attachments, attached.message.attachments);
  await api(`/cases/${id}/conversation/messages`, { token: a, method: 'POST', body: { ...attachmentInput, attachment_ids: [uploaded.id, image.id] }, status: 409 });
  await api(`/cases/${id}/conversation/messages`, { token: a, method: 'POST', body: { client_message_id: randomUUID(), text: '', attachment_ids: [image.id, image.id] }, status: 400 });
  await api(`/cases/${id}/conversation/messages`, { token: a, method: 'POST', body: { client_message_id: randomUUID(), text: '', attachment_ids: [image.id], settings: { approval: 'ask', model: 'deepseek-v4-pro', effort: 'high' } }, status: 400 });
  assert.deepEqual((await api(`/cases/${id}/conversation`, { token: a })).items.find(item => item.id === attached.message.id).attachments.map(item => item.id), attachmentInput.attachment_ids);
  for (const [token, caseId] of [[a, plain.id], [c, (await api('/conversations', { token: c, method: 'POST', body: { request_id: randomUUID() }, status: 201 })).id]]) {
    await api(`/cases/${caseId}/conversation/messages`, { token, method: 'POST', body: { client_message_id: randomUUID(), text: 'Try foreign attachment', attachment_ids: [image.id] }, status: 400 });
  }
  note('message attachments persist in order; replay, model capability and case/owner scope are enforced');
  await testMemoryTransaction({ api, db, actor, token: a, storageToken: newClaim.storage_token, session: sid, caseId: id, messageId: message.message.id });
  note('Agent memory and permission audit share a transaction; retries and memory capacity preserve existing status codes');
  const memory = await api('/memory', { token: a, method: 'POST', body: { request_id: randomUUID(), text: 'Prefer totals in units.' }, status: 201 });
  assert.equal((await api('/memory', { token: c })).items.length, 0); await api(`/memory/${memory.id}`, { token: c, method: 'DELETE', status: 404 });
  await stop(); await start();
  assert.equal((await api(`/internal/agent-sessions/${sid}`, { token: newClaim.storage_token })).state.messages.length, 1);
  assert.deepEqual((await api(`/internal/agent-sessions/${sid}/documents/todos`, { token: newClaim.storage_token })).payload, todo.payload);
  assert.equal((await api('/memory', { token: a })).items[0].id, memory.id);
  note('PostgreSQL-only session/message/todo/memory recovery, CAS and token fencing after restart');
  const operation = randomUUID(); loseCreate = true;
  const sandbox = await api(`/cases/${id}/sandbox`, { token: a, method: 'POST', body: { request_id: operation } });
  assert.equal(sandbox.operation.state, 'provisioned'); assert.equal(sandbox.operation.guest_ready, true); assert.equal(remote.metadata.erp_user, actor.userId);
  await api(`/cases/${id}/sandbox`, { token: a, method: 'POST', body: { request_id: operation } }); assert.equal(creates, 1);
  await api(`/cases/${id}/sandbox/${operation}`, { token: c, method: 'DELETE', status: 404 });
  uncertainDelete = true; assert.equal((await api(`/cases/${id}/sandbox/${operation}`, { token: a, method: 'DELETE' })).operation.state, 'releasing');
  uncertainDelete = false; assert.equal((await api(`/cases/${id}/sandbox`, { token: a })).operation.state, 'released');
  note('Cube lost-create reconciliation, no duplicate creation, user metadata, asynchronous deletion');

  const pendingCase = await api('/cases', { token: a, method: 'POST', body: create, status: 201 });
  const pendingMessage = await api(`/cases/${pendingCase.id}/conversation/messages`, { token: a, method: 'POST', body: { client_message_id: randomUUID(), text: 'orphan' }, status: 201 });
  const pendingSid = (await api(`/cases/${pendingCase.id}/conversation`, { token: a })).session_id;
  const orphanClaim = await api(`/agent-sessions/${pendingSid}/claim`, { token: a, method: 'POST', body: {} });
  await db.query("INSERT INTO agent_runs(message_id,tenant_id,case_id,state,text,tools) VALUES($1,$2,$3,'running','Saved prefix','[{\"id\":\"t1\",\"name\":\"case_read\",\"state\":\"running\"}]')", [pendingMessage.message.id, actor.tenantId, pendingCase.id]);
  await stop();
  env.DEEPSEEK_API_KEY = 'synthetic-test-only';
  env.AGENT_RUNTIME_BINARY = fixtureBinary;
  await start();
  async function finished(caseId, messageId) {
    for (let i = 0; i < 120; i++) {
      const item = (await api(`/cases/${caseId}/conversation`, { token: a })).items.find(x => x.id === messageId);
      if (item?.response && ['completed', 'failed', 'cancelled'].includes(item.response.state)) return item.response;
      await new Promise(resolve => setTimeout(resolve, 50));
    }
    throw new Error(`Run did not finish: ${logs}`);
  }
  const orphan = await finished(pendingCase.id, pendingMessage.message.id);
  assert.equal(orphan.state, 'failed'); assert.equal(orphan.text, 'Saved prefix'); assert.equal(orphan.tools[0].state, 'failed');
  await api(`/internal/agent-sessions/${pendingSid}`, { token: orphanClaim.storage_token, status: 401 });

  const runCase = await api('/cases', { token: a, method: 'POST', body: create, status: 201 });
  const runPath = `/cases/${runCase.id}/conversation/messages`;
  for (const count of [1, 2, 4, 8]) {
    const parallelCases = await Promise.all(Array.from({ length: count }, () => api('/cases', { token: a, method: 'POST', body: create, status: 201 })));
    const parallelMessages = await Promise.all(parallelCases.map((item, index) => api(`/cases/${item.id}/conversation/messages`, { token: a, method: 'POST', body: { client_message_id: randomUUID(), text: `parallel:${count}:${index}` }, status: 201 })));
    const parallelResults = await Promise.all(parallelCases.map((item, index) => finished(item.id, parallelMessages[index].message.id)));
    assert(parallelResults.every(result => result.state === 'completed'), `${JSON.stringify(parallelResults)}\n${logs}`);
    const timings = (await db.query('SELECT r.message_id,m.created_at,r.started_at,r.first_token_at,r.finished_at FROM agent_runs r JOIN case_conversation_messages m ON m.id=r.message_id WHERE r.message_id=ANY($1::bigint[])', [parallelMessages.map(item => item.message.id)])).rows;
    assert.equal(timings.length, count);
    const ms = value => new Date(value).getTime();
    assert(timings.every(item => ms(item.created_at) <= ms(item.started_at) && ms(item.started_at) <= ms(item.first_token_at) && ms(item.first_token_at) <= ms(item.finished_at)));
    const events = timings.flatMap(item => [[ms(item.started_at), 1], [ms(item.finished_at), -1]]).sort((a, b) => a[0] - b[0] || a[1] - b[1]);
    let active = 0; let peak = 0;
    for (const [, change] of events) { active += change; peak = Math.max(peak, active); }
    assert(peak <= 4 && peak >= Math.min(count, 3), `Unexpected worker overlap: ${JSON.stringify(timings)}`);
    const percentile = (values, ratio) => { const ordered = [...values].sort((a, b) => a - b); return ordered[Math.ceil(ordered.length * ratio) - 1]; };
    const queue = timings.map(item => ms(item.started_at) - ms(item.created_at));
    const ttft = timings.map(item => ms(item.first_token_at) - ms(item.created_at));
    const total = timings.map(item => ms(item.finished_at) - ms(item.created_at));
    console.log(`PERF ${JSON.stringify({ concurrency: count, peak, queue_p50_ms: percentile(queue, .5), queue_p95_ms: percentile(queue, .95), ttft_p50_ms: percentile(ttft, .5), ttft_p95_ms: percentile(ttft, .95), total_p50_ms: percentile(total, .5), total_p95_ms: percentile(total, .95), batch_wall_ms: Math.max(...timings.map(item => ms(item.finished_at))) - Math.min(...timings.map(item => ms(item.created_at))) })}`);
  }
  note('one API worker bounds independent-case runtime concurrency at four while retaining case-scoped locking');
  second = spawn(binary, [], { env: { ...env, API_PORT: '0' }, stdio: 'ignore' });
  const turnInput = { client_message_id: randomUUID(), text: 'read' };
  const sent = await Promise.all([0, 1].map(() => api(runPath, { token: a, method: 'POST', body: turnInput, status: [200, 201] })));
  assert.equal(sent[0].message.id, sent[1].message.id);
  await api(runPath, { token: a, method: 'POST', body: { client_message_id: randomUUID(), text: 'busy' }, status: 409 });
  const success = await finished(runCase.id, sent[0].message.id);
  assert.equal(success.state, 'completed', logs); assert.equal(success.text, `Scoped reply for ${runCase.id}`);
  assert.equal(success.steps[0].state, 'completed'); assert.equal(success.tools[0].state, 'completed');
  assert.equal((await db.query('SELECT loaded_skills FROM agent_runs WHERE message_id=$1', [sent[0].message.id])).rows[0].loaded_skills.length, 1);
  const slow = await api(runPath,{token:a,method:'POST',body:{client_message_id:randomUUID(),text:'slow'},status:201});
  for (let i=0;i<100;i++) { const r=(await db.query('SELECT state FROM agent_runs WHERE message_id=$1',[slow.message.id])).rows[0]; if(r.state==='running') break; await new Promise(resolve=>setTimeout(resolve,50)); }
  const cancelPath = `/cases/${runCase.id}/conversation/cancel`;
  await api(cancelPath,{token:c,method:'POST',body:{message_id:slow.message.id},status:404});
  for(let i=0;i<2;i++) await api(cancelPath,{token:a,method:'POST',body:{message_id:slow.message.id}});
  assert.equal((await finished(runCase.id,slow.message.id)).state,'cancelled');
  assert.equal((await db.query("SELECT count(*)::int AS n FROM audit_events WHERE case_id=$1 AND event_type='agent.cancel_requested'",[runCase.id])).rows[0].n,1);
  note('running cancellation terminates and persists once; other users cannot cancel');
  for (const text of ['interrupted', 'unknown tool', 'wrong skill', 'continue']) {
    const msg = await api(runPath, { token: a, method: 'POST', body: { client_message_id: randomUUID(), text }, status: 201 });
    const result = await finished(runCase.id, msg.message.id);
    assert.equal(result.state, text === 'continue' ? 'completed' : 'failed');
    if (text === 'interrupted') { assert.equal(result.text, 'Saved partial reply'); assert.equal(result.tools[0].state, 'failed'); assert.equal(result.steps.at(-1).state, 'failed'); }
  }
  second.kill('SIGTERM'); await once(second, 'exit'); second = null;
  await stop(); await start();
  const newest = await api(`/cases/${runCase.id}/conversation?limit=2`, { token: a });
  assert.equal(newest.items.length, 2); assert(newest.next_before);
  const older = await api(`/cases/${runCase.id}/conversation?limit=2&before=${newest.next_before}`, { token: a });
  assert.equal(older.items.length, 2); assert(!older.items.some(x => newest.items.some(y => x.id === y.id)));
  assert.equal(newest.items.at(-1).response.state, 'completed');
  note('two real worker processes serialize; partial failures, unpublished tools and mismatched skills fail safely; orphan fencing and pagination survive restart');

  const shutdownMessage = await api(runPath, { token: a, method: 'POST', body: { client_message_id: randomUUID(), text: 'checkpoint shutdown' }, status: 201 });
  for (let i = 0; i < 100; i++) {
    const row = (await db.query('SELECT text FROM agent_runs WHERE message_id=$1', [shutdownMessage.message.id])).rows[0];
    if (row.text) break;
    await new Promise(resolve => setTimeout(resolve, 50));
    assert(i < 99, 'Shutdown fixture did not start');
  }
  await stop(); await start();
  const stopped = await finished(runCase.id, shutdownMessage.message.id);
  assert.equal(stopped.state, 'failed'); assert.equal(stopped.text, 'Saved during shutdown');
  const shutdownSid = (await api(`/cases/${runCase.id}/conversation`, { token: a })).session_id;
  assert.equal((await db.query("SELECT payload->>'text' AS text FROM agent_session_documents WHERE session_id=$1 AND kind='stream_checkpoint'", [shutdownSid])).rows[0].text, stopped.text);
  note('API shutdown keeps storage listener and lease alive until the runtime checkpoint is committed');

  if (process.env.VERIFY_LONG_TURN === '1') {
    const startTime = Date.now();
    const long = await api(runPath, { token: a, method: 'POST', body: { client_message_id: randomUUID(), text: 'long' }, status: 201 });
    let response;
    for (let i = 0; i < 175; i++) {
      response = (await api(`/cases/${runCase.id}/conversation`, { token: a })).items.find(item => item.id === long.message.id)?.response;
      if (response && ['completed', 'failed'].includes(response.state)) break;
      await new Promise(resolve => setTimeout(resolve, 1000));
    }
    assert.equal(response.state, 'completed', JSON.stringify(response)); assert(Date.now() - startTime >= 151000);
    note('a real worker turn lasting over 150 seconds completes with the default deadline disabled');
  }

  {
  const approvalCase = await api('/conversations',{token:a,method:'POST',body:{request_id:randomUUID()},status:201});
  const approvalPath=`/cases/${approvalCase.id}`;
  const submitAction=async (text,approval='ask') => (await api(`${approvalPath}/conversation/messages`,{token:a,method:'POST',body:{client_message_id:randomUUID(),text:`memory:${text}`,settings:{approval,model:'deepseek-flash',effort:'none'}},status:201})).message.id;
  const waiting=async mid => {
    for(let i=0;i<120;i++) {const item=(await api(`${approvalPath}/conversation`,{token:a})).items.find(x=>x.id===mid);if(item?.response?.state==='waiting')return item.response;await new Promise(resolve=>setTimeout(resolve,50));}
    throw Error(`Approval did not suspend: ${logs}`);
  };
  const memoryCount=async text => Number((await db.query('SELECT count(*) FROM user_memories WHERE owner_id=$1 AND text=$2',[actor.userId,text])).rows[0].count);
  let actionMid=await submitAction('Prefer metric units.');
  let paused=await waiting(actionMid);let permission=paused.permissions[0];
  assert.equal(permission.state,'pending');assert.equal(await memoryCount('Prefer metric units.'),0);
  await stop();await start();paused=await waiting(actionMid);assert.equal(paused.permissions[0].id,permission.id);
  const decisionPath=`${approvalPath}/tool-permissions/${permission.id}`;
  const decision={decision:'allowed',arguments_digest:permission.arguments_digest};
  await api(decisionPath,{token:c,method:'POST',body:decision,status:404});
  await api(decisionPath,{token:a,method:'POST',body:{...decision,arguments_digest:'changed'},status:409});
  await Promise.all([0,1].map(()=>api(decisionPath,{token:a,method:'POST',body:decision})));
  let result=await finished(approvalCase.id,actionMid);assert.equal(result.state,'completed',JSON.stringify(result));assert.equal(result.text,'saved');assert.equal(result.permissions[0].state,'executed');assert.equal(await memoryCount('Prefer metric units.'),1);
  assert.equal(result.tools.length,1);assert.equal(result.tools[0].state,'completed');
  await api(decisionPath,{token:a,method:'POST',body:decision});
  await api(decisionPath,{token:a,method:'POST',body:{...decision,decision:'denied'},status:409});
  for(const [text,mode,status] of [['Full access fixture','full','executed'],['Remember allow','auto','executed'],['Remember deny','auto','denied'],['Remember malformed','auto','denied']]) {
    const mid=await submitAction(text,mode);const response=await finished(approvalCase.id,mid);
    assert.equal(response.state,'completed',JSON.stringify(response));assert.equal(response.permissions[0].state,status);assert.equal(await memoryCount(text),status==='executed'?1:0);
    assert.equal(response.permissions[0].decided_by,mode==='auto'?'reviewer':'full');
  }
  for(const scenario of ['reject','expire','cancel']) {
    actionMid=await submitAction(scenario);paused=await waiting(actionMid);permission=paused.permissions[0];
    if(scenario==='reject') await api(`${approvalPath}/tool-permissions/${permission.id}`,{token:a,method:'POST',body:{decision:'denied',arguments_digest:permission.arguments_digest}});
    if(scenario==='expire') await db.query("UPDATE agent_tool_permissions SET expires_at=now()-interval '1 second' WHERE id=$1",[permission.id]);
    if(scenario==='cancel') await api(`${approvalPath}/conversation/cancel`,{token:a,method:'POST',body:{message_id:actionMid}});
    result=await finished(approvalCase.id,actionMid);assert.equal(result.state,scenario==='cancel'?'cancelled':'completed');assert.equal(await memoryCount(scenario),0);
  }
  note('durable tool approval pause/restart/resume, exact digest, owner isolation, duplicate decisions, full execution, independent auto review and fail-closed rejection/expiry/cancel');
  }

  await testBusinessActions({ api, db, actor, token: a, otherToken: c, create, proposal, finished, stop, start });
  note('business action modes, exact-version human approval, live case reads, restart continuation, role and evidence isolation, duplicate replay, rejection and stale-version checks');

  const compactPath = `/cases/${runCase.id}/compactions`;
  for (const token of [b, c]) await api(compactPath, { token, method: 'POST', body: { request_id: randomUUID() }, status: 404 });
  const compactInput = { request_id: randomUUID() };
  const compact = await api(compactPath, { token: a, method: 'POST', body: compactInput, status: 201 });
  assert.equal((await api(compactPath, { token: a, method: 'POST', body: compactInput })).message.id, compact.message.id);
  const compacted = await finished(runCase.id, compact.message.id);
  assert.equal(compacted.state, 'completed'); assert.equal(compacted.compactions.length, 1);
  assert.equal(compacted.compactions[0].tokens_saved, 9000);
  const originalCompactSettings = compact.message.settings;
  const changedCompactSettings = { approval: 'auto', model: 'deepseek-v4-pro', effort: 'max' };
  await api(`/cases/${runCase.id}/conversation/settings`, { token: a, method: 'PATCH', body: changedCompactSettings });
  const compactRetry = await api(compactPath, { token: a, method: 'POST', body: compactInput });
  assert.equal(compactRetry.message.id, compact.message.id);
  assert.deepEqual(compactRetry.message.settings, originalCompactSettings);
  assert.deepEqual((await api(`/cases/${runCase.id}/conversation`, { token: a })).settings, changedCompactSettings);
  await api(runPath, { token: a, method: 'POST', body: { client_message_id: compactInput.request_id, text: 'Compress conversation context' }, status: 409 });
  for (const part_number of ['FAIL-COMPACTION', 'INVALID-COMPACTION']) {
    const failCase = await api('/cases', { token: a, method: 'POST', body: { ...create, part_number }, status: 201 });
    const failInput = { request_id: randomUUID() };
    const failPath = `/cases/${failCase.id}/compactions`;
    const failure = await api(failPath, { token: a, method: 'POST', body: failInput, status: 201 });
    const result = await finished(failCase.id, failure.message.id);
    assert.equal(result.state, 'failed'); assert.equal(result.compactions.length, 0);
    assert.equal(result.error, 'Context compression failed. You can retry compression.');
    assert.equal((await api(failPath, { token: a, method: 'POST', body: failInput })).message.id, failure.message.id);
  }
  await stop(); await start();
  const recoveredCompact = (await api(`/cases/${runCase.id}/conversation`, { token: a })).items.find(item => item.id === compact.message.id);
  assert.equal(recoveredCompact.operation, 'compact'); assert.deepEqual(recoveredCompact.response.compactions, compacted.compactions);
  note('compaction isolation, operation idempotency, failure without false success, and metrics after restart');

  await stop();
  env.AGENT_EXECUTION_BACKEND = 'cube';
  env.CUBE_RUNTIME_SHARED_SECRET = 'synthetic-cube-runtime-secret-with-32-chars';
  loseCreate = false;
  cubeTurnCompleted = false;
  await start();
  const cubeCase = await api('/cases', { token: a, method: 'POST', body: create, status: 201 });
  const beforeCubeCreates = creates;
  const cubeMessage = await api(`/cases/${cubeCase.id}/conversation/messages`, { token: a, method: 'POST', body: { client_message_id: randomUUID(), text: 'cube streaming' }, status: 201 });
  let cubePartial;
  for (let i = 0; i < 400; i++) {
    cubePartial = (await db.query('SELECT state,text,first_token_at FROM agent_runs WHERE message_id=$1', [cubeMessage.message.id])).rows[0];
    if (cubePartial?.text) break;
    await new Promise(resolve => setTimeout(resolve, 25));
  }
  const cubeState = await db.query('SELECT state,guest_ready,error_code FROM case_sandboxes WHERE case_id=$1 ORDER BY created_at DESC LIMIT 1', [cubeCase.id]);
  assert.equal(cubePartial?.text, 'Streamed from Cube', `${JSON.stringify({ run: cubePartial, sandbox: cubeState.rows[0], created: creates - beforeCubeCreates })}\n${logs}`);
  assert.equal(cubePartial.state, 'running');
  assert(cubePartial.first_token_at);
  assert.equal(cubeTurnCompleted, false);
  const cubeResult = await finished(cubeCase.id, cubeMessage.message.id);
  assert.equal(cubeResult.state, 'completed');
  assert.equal(cubeResult.text, 'Streamed from Cube');
  assert.equal(creates, beforeCubeCreates + 1);
  const cubeSandbox = (await api(`/cases/${cubeCase.id}/sandbox`, { token: a })).operation;
  assert.equal(cubeSandbox.guest_ready, true);
  assert.equal((await db.query('SELECT mcp_session_id FROM case_sandboxes WHERE id=$1', [cubeSandbox.id])).rows[0].mcp_session_id.startsWith('mcp_'), true);
  await api(`/cases/${cubeCase.id}/sandbox/${cubeSandbox.id}`, { token: a, method: 'DELETE' });
  await stop();
  env.AGENT_EXECUTION_BACKEND = 'local';
  await start();
  note('Cube worker opt-in auto-provisions a case resident and saves streamed text before completion');

  await stop(); env.DEEPSEEK_API_KEY = ''; await start();
  const denied = await api(runPath, { token: a, method: 'POST', body: { client_message_id: randomUUID(), text: 'revoked' }, status: 201 });
  await db.query('INSERT INTO agent_runs(message_id,tenant_id,case_id) VALUES($1,$2,$3)', [denied.message.id, actor.tenantId, runCase.id]);
  await db.query("UPDATE users SET roles=ARRAY['approver'] WHERE id=$1", [actor.userId]);
  await stop(); env.DEEPSEEK_API_KEY = 'synthetic-test-only'; await start();
  assert.equal((await finished(runCase.id, denied.message.id)).state, 'failed');
  assert.equal((await db.query('SELECT skill_catalog_hash FROM agent_runs WHERE message_id=$1', [denied.message.id])).rows[0].skill_catalog_hash, null);
  note('queued work rechecks authorization before invoking the runtime');
  await api(`/memory/${memory.id}`, { token: a, method: 'DELETE', status: 204 });
  assert.equal((await db.query('SELECT status FROM user_memories WHERE id=$1', [memory.id])).rows[0].status, 'retracted');
  assert.deepEqual((await db.query("SELECT operation,state FROM memory_outbox WHERE memory_version_id=$1 AND operation='retract'", [memory.id])).rows, []);
  assert.deepEqual((await db.query("SELECT event_type FROM memory_audit_events WHERE memory_version_id=$1 ORDER BY id", [memory.id])).rows, [{ event_type: 'memory.created' }, { event_type: 'memory.retracted' }]);
  await api('/auth/logout', { token: a, method: 'POST', status: 204 }); await api('/auth/me', { token: a, status: 401 });
  note('logout revocation and scoped memory deletion');
} finally {
  if (second && second.exitCode === null) { second.kill('SIGTERM'); await once(second, 'exit'); }
  await stop(); await new Promise(resolve => cube.close(resolve)); await db.end();
  await admin.query(`DROP DATABASE ${name} WITH (FORCE)`); await admin.end();
  await rm(fixtureDir, { recursive: true, force: true });
}
