import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';

export async function testBusinessActions({ api, db, actor, token, otherToken, create, proposal, finished, stop, start }) {
  const read = id => api(`/cases/${id}`, { token });
  const send = async (id, tool, args, approval = 'full') => (await api(`/cases/${id}/conversation/messages`, { token, method: 'POST', status: 201, body: { client_message_id: randomUUID(), text: `action:${JSON.stringify({ tool, arguments: args, nonce: randomUUID() })}`, settings: { approval, model: 'deepseek-flash', effort: 'none' } } })).message.id;
  const waiting = async (id, mid) => {
    for (let i = 0; i < 180; i++) {
      const response = (await api(`/cases/${id}/conversation`, { token })).items.find(item => item.id === mid)?.response;
      assert.notEqual(response?.state, 'failed', JSON.stringify(response));
      if (response?.state === 'waiting') return response.permissions.at(-1);
      await new Promise(resolve => setTimeout(resolve, 50));
    }
    throw Error('Business action did not suspend');
  };
  const decide = (id, p, decision = 'allowed', status = 200, override = {}) => api(`/cases/${id}/tool-permissions/${p.id}`, { token, method: 'POST', status, body: { decision, arguments_digest: p.arguments_digest }, ...override });
  const attach = async id => {
    const documentId = randomUUID();
    await db.query("INSERT INTO documents(id,tenant_id,owner_id,case_id,object_key,filename,content_type,bytes,sha256,state) VALUES($1,$2,$3,$4,$5,'synthetic.txt','text/plain',10,$6,'ready')", [documentId, actor.tenantId, actor.userId, id, `tests/${documentId}`, 'c'.repeat(64)]);
    return documentId;
  };
  for (const mode of ['ask', 'auto', 'full']) {
    const { id } = await api('/conversations', { token, method: 'POST', status: 201, body: { request_id: randomUUID() } });
    let mid = await send(id, 'case_set_context', { expected_case_version: 1, context: create }, mode);
    if (mode === 'ask') {
      const p = await waiting(id, mid);
      assert.equal((await read(id)).kind, 'conversation');
      await decide(id, p, 'allowed', 404, { token: otherToken });
      await decide(id, p, 'allowed', 409, { body: { decision: 'allowed', arguments_digest: 'changed' } });
      await stop(); await start();
      assert.equal((await waiting(id, mid)).id, p.id);
      await Promise.all([decide(id, p), decide(id, p)]);
    }
    let result = await finished(id, mid);
    assert.equal(result.state, 'completed', JSON.stringify(result));
    assert.equal(result.permissions[0].state, 'executed');
    assert.equal((await read(id)).version, 2);
    const documentId = await attach(id);
    const args = { ...proposal, expected_case_version: 2, evidence: [{ ...proposal.evidence[0], document_id: documentId }] };
    mid = await send(id, 'proposal_create', args, mode);
    if (mode === 'ask') {
      const p = await waiting(id, mid);
      assert.equal((await read(id)).proposals.length, 0);
      await decide(id, p);
    }
    result = await finished(id, mid);
    assert.equal(result.state, 'completed', JSON.stringify(result));
    const detail = await read(id);
    assert.equal(detail.proposals.length, 1);
    const saved = detail.proposals[0];
    assert.equal(detail.version, 3);
    const approvalArgs = { proposal_id: saved.id, proposal_digest: saved.digest, expected_case_version: saved.case_version };
    mid = await send(id, 'request_proposal_approval', approvalArgs, mode);
    const p = await waiting(id, mid);
    assert.equal(p.mode, 'ask');
    assert.deepEqual(p.proposal.actions, saved.actions);
    assert.equal((await read(id)).approvals.length, 0);
    await decide(id, p, 'allowed', 400);
    await db.query("UPDATE users SET roles=ARRAY['planner'] WHERE id=$1", [actor.userId]);
    const request = { token, method: 'POST', status: 201, headers: { 'idempotency-key': randomUUID() }, body: { ...approvalArgs, decision: mode === 'auto' ? 'rejected' : 'approved' } };
    await api(`/cases/${id}/approvals`, { ...request, status: 403 });
    assert.equal((await waiting(id, mid)).can_approve, false);
    await db.query("UPDATE users SET roles=ARRAY['planner','approver'] WHERE id=$1", [actor.userId]);
    await api(`/cases/${id}/approvals`, { ...request, token: otherToken, status: 404 });
    await api(`/cases/${id}/approvals`, { ...request, status: 409, body: { ...request.body, proposal_digest: '0'.repeat(64) } });
    if (mode === 'full') { await stop(); await start(); assert.equal((await waiting(id, mid)).id, p.id); }
    await Promise.all([0, 1].map(() => api(`/cases/${id}/approvals`, { ...request, status: [200, 201] })));
    result = await finished(id, mid);
    assert.equal(result.state, 'completed', JSON.stringify(result));
    assert.equal(result.text, request.body.decision);
    assert.equal(result.permissions[0].result.erp_executed, false);
    assert.equal((await read(id)).approvals.length, 1);
    assert.equal((await read(id)).version, 4);
    assert.equal((await api(`/cases/${id}/conversation`, { token })).case_version, 4);
  }
  const { id } = await api('/cases', { token, method: 'POST', status: 201, body: create });
  await api(`/cases/${id}/proposals`, { token, method: 'POST', status: 400, body: proposal });
  const documentId = await attach(id);
  const args = { ...proposal, evidence: [{ ...proposal.evidence[0], document_id: documentId }] };
  let mid = await send(id, 'proposal_create', args, 'ask');
  let p = await waiting(id, mid);
  await decide(id, p, 'denied');
  assert.equal((await finished(id, mid)).text, 'denied');
  assert.equal((await read(id)).proposals.length, 0);
  mid = await send(id, 'proposal_create', args, 'ask');
  p = await waiting(id, mid);
  await api(`/cases/${id}/proposals`, { token, method: 'POST', status: 201, body: args });
  await decide(id, p);
  assert.equal((await finished(id, mid)).state, 'failed');
  assert.equal((await read(id)).proposals.length, 1);
  const saved = (await read(id)).proposals[0];
  mid = await send(id, 'request_proposal_approval', { proposal_id: saved.id, proposal_digest: saved.digest, expected_case_version: saved.case_version });
  await waiting(id, mid);
  await api(`/cases/${id}/proposals`, { token, method: 'POST', status: 201, body: { ...args, expected_case_version: 2 } });
  assert.equal((await finished(id, mid)).text, 'denied');
  assert.equal((await read(id)).approvals.length, 0);
}
