import assert from 'node:assert/strict';
import { randomUUID, createHash } from 'node:crypto';

// S3 transport fixture only; this does not certify a real RustFS deployment.
export function originalStorageFixture() {
  const objects = new Map();
  const control = { failPut: false, corruptGet: false, puts: 0 };
  return {
    control,
    handle(req, res, bytes) {
      if (!req.url.startsWith('/fixture-originals/')) return false;
      if (req.method === 'PUT') {
        control.puts++;
        if (control.failPut) { res.statusCode = 503; res.end('unavailable'); return true; }
        objects.set(req.url, bytes);
        res.statusCode = 200; res.end(); return true;
      }
      if (req.method === 'GET' && objects.has(req.url)) {
        res.statusCode = 200; res.end(control.corruptGet ? Buffer.from('corrupted') : objects.get(req.url)); return true;
      }
      res.statusCode = 404; res.end(); return true;
    },
  };
}

export async function testOriginalBoundaries({ api, db, actor, token, otherTokens, caseId, control }) {
  const path = `/cases/${caseId}/documents`;
  const bytes = Buffer.from('Synthetic document with 中文, preserved bytes.');
  const upload = { request_id: randomUUID(), filename: 'note.txt', content_type: 'text/plain', data_base64: bytes.toString('base64') };
  const send = (body, status = 201, auth = token) => api(path, { token: auth, method: 'POST', body, status });
  for (const other of otherTokens) await send(upload, 404, other);
  assert.equal(control.puts, 0);
  const doc = await send(upload);
  assert.equal(doc.sha256, createHash('sha256').update(bytes).digest('hex'));
  assert.equal(doc.state, 'ready');
  assert.equal(doc.object_key, undefined); assert.equal(doc.owner_id, undefined); assert.equal(doc.tenant_id, undefined);
  assert.equal((await send(upload, 200)).id, doc.id);
  assert.equal(control.puts, 1);
  for (const change of [{ filename: 'changed.txt' }, { filename: 'changed.pdf', content_type: 'application/pdf', data_base64: Buffer.from('%PDF-test').toString('base64') }, { data_base64: Buffer.from('changed').toString('base64') }]) {
    await send({ ...upload, ...change }, 409);
  }
  assert.deepEqual(await api(`/documents/${doc.id}`, { token, raw: true }), bytes);
  for (const other of otherTokens) await api(`/documents/${doc.id}`, { token: other, status: 404 });
  control.corruptGet = true;
  await api(`/documents/${doc.id}`, { token, status: 503 });
  control.corruptGet = false;
  assert.deepEqual(await api(`/documents/${doc.id}`, { token, raw: true }), bytes);
  const retry = { ...upload, request_id: randomUUID() };
  control.failPut = true;
  await send(retry, 503);
  assert.equal((await db.query('SELECT state FROM documents WHERE id=$1', [retry.request_id])).rows[0].state, 'failed');
  control.failPut = false;
  assert.equal((await send(retry)).state, 'ready');
  const listed = (await api(path, { token })).items;
  assert(listed.some(x => x.id === retry.request_id));
  assert(listed.every(x => x.object_key === undefined && x.owner_id === undefined && x.tenant_id === undefined));
  return doc;
}

export async function testMemoryTransaction({ api, db, actor, token, storageToken, session, caseId, messageId }) {
  // API currently has no model key; a controlled running row cannot be consumed by a worker.
  const settings = { approval: 'full', model: 'deepseek-flash', effort: 'none' };
  await db.query("INSERT INTO agent_runs(message_id,tenant_id,case_id,state,settings) VALUES($1,$2,$3,'running',$4)", [messageId, actor.tenantId, caseId, settings]);
  const call = { tool_call_id: `atomic-${randomUUID()}`, tool: 'memory_save', arguments: { text: 'Atomic memory fixture' } };
  const path = `/internal/agent-sessions/${session}/actions`;
  await db.query("CREATE FUNCTION reject_memory_audit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.event_type='agent.action_executed' THEN RAISE EXCEPTION 'synthetic audit failure'; END IF; RETURN NEW; END $$");
  await db.query('CREATE TRIGGER reject_memory_audit BEFORE INSERT ON audit_events FOR EACH ROW EXECUTE FUNCTION reject_memory_audit()');
  try {
    await api(path, { token: storageToken, method: 'POST', body: call, status: 500 });
    assert.equal((await db.query('SELECT count(*)::int AS n FROM user_memories WHERE owner_id=$1 AND text=$2', [actor.userId, call.arguments.text])).rows[0].n, 0);
    assert.equal((await db.query('SELECT count(*)::int AS n FROM agent_tool_permissions WHERE session_id=$1 AND tool_call_id=$2', [session, call.tool_call_id])).rows[0].n, 0);
  } finally {
    await db.query('DROP TRIGGER reject_memory_audit ON audit_events'); await db.query('DROP FUNCTION reject_memory_audit()');
  }
  const saved = await api(path, { token: storageToken, method: 'POST', body: call });
  assert.equal(saved.status, 'saved');
  const savedRow = (await db.query('SELECT status,scope_kind,scope_id,source_kind FROM user_memories WHERE id=$1', [saved.memory_id])).rows[0];
  assert.deepEqual(savedRow, { status: 'active', scope_kind: 'user', scope_id: actor.userId, source_kind: 'agent' });
  assert.deepEqual((await db.query('SELECT operation,state FROM memory_outbox WHERE memory_version_id=$1 ORDER BY operation', [saved.memory_id])).rows, []);
  assert.deepEqual((await db.query('SELECT event_type,payload->>\'source_kind\' AS source_kind FROM memory_audit_events WHERE memory_version_id=$1', [saved.memory_id])).rows, [{ event_type: 'memory.created', source_kind: 'agent' }]);
  assert.deepEqual(await api(path, { token: storageToken, method: 'POST', body: call }), saved);
  assert.equal((await db.query('SELECT count(*)::int AS n FROM user_memories WHERE owner_id=$1 AND text=$2', [actor.userId, call.arguments.text])).rows[0].n, 1);
  const memoryIds = [];
  try {
    const count = (await db.query('SELECT count(*)::int AS n FROM user_memories WHERE owner_id=$1', [actor.userId])).rows[0].n;
    for (let i = count; i < 50; i++) {
      const id = randomUUID(); memoryIds.push(id);
      await db.query('INSERT INTO user_memories(id,tenant_id,owner_id,text) VALUES($1,$2,$3,$4)', [id, actor.tenantId, actor.userId, `limit fixture ${i}`]);
    }
    const full = await api(path, { token: storageToken, method: 'POST', body: { ...call, tool_call_id: randomUUID() }, status: 409 });
    assert.equal(full.error, 'memory_limit');
    await api('/memory', { token, method: 'POST', body: { request_id: randomUUID(), text: 'over limit' }, status: 400 });
  } finally {
    await db.query('DELETE FROM user_memories WHERE id=ANY($1::uuid[])', [[saved.memory_id, ...memoryIds]]);
    await db.query('DELETE FROM agent_runs WHERE message_id=$1', [messageId]);
  }
}
