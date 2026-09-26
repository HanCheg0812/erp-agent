import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';

const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

export function cubeQuantityFixture() {
  const fixture = { creates: 0, executions: 0, mode: 'ok', blockDelete: false, instances: new Map() };
  let hiddenLists = 0;
  fixture.handle = async (req, res, bytes) => {
    const body = bytes.length ? JSON.parse(bytes.toString('utf8')) : null;
    const reply = value => { res.setHeader('content-type', 'application/json'); res.end(JSON.stringify(value)); return true; };
    if (req.url === '/templates/quantity-test') {
      return reply({ status: 'READY', replicas: [{ status: 'READY', node_ip: '127.0.0.1' }] });
    }
    if (req.url === '/sandboxes' && req.method === 'POST' && body.templateID === 'quantity-test') {
      fixture.creates++;
      assert.equal(body.allow_internet_access, false);
      assert.deepEqual(body.network.allow_out, []);
      assert.equal(body.timeout, 180);
      assert(Object.values(body.metadata).every(value => typeof value === 'string'));
      const instance = { sandboxID: `quantity-${fixture.creates}`, templateID: body.templateID, metadata: body.metadata };
      fixture.instances.set(instance.sandboxID, instance);
      if (fixture.mode === 'lost-create') { req.socket.destroy(); return true; }
      if (fixture.mode === 'late-create') { hiddenLists = 2; res.statusCode = 500; return reply({ error: 'delayed creation' }); }
      return reply(instance);
    }
    if (req.url.startsWith('/sandboxes?metadata=erp_tool_operation')) {
      if (hiddenLists > 0) { hiddenLists--; return reply([]); }
      const operation = new URL(req.url, 'http://fixture').searchParams.get('metadata').split('=')[1];
      return reply([...fixture.instances.values()].filter(item => item.metadata.erp_tool_operation === operation));
    }
    if (req.url.startsWith('/sandboxes/quantity-')) {
      const id = req.url.split('/').at(-1);
      if (req.method === 'DELETE') {
        if (!fixture.blockDelete) fixture.instances.delete(id);
        res.statusCode = 204; res.end(); return true;
      }
      if (!fixture.instances.has(id)) res.statusCode = 404;
      return reply(fixture.instances.get(id) ?? null);
    }
    if (req.url.startsWith('/sandbox/quantity-') && req.url.endsWith('/execute')) {
      fixture.executions++;
      assert.equal(body.language, 'python');
      assert.equal(body.env_vars, null);
      const values = JSON.parse(body.code.split('\n')[1].slice('_values = '.length));
      const total = fixture.mode === 'wrong-result' ? -1 : values.reduce((sum, value) => sum + value, 0);
      await sleep(50);
      res.setHeader('content-type', 'application/x-ndjson');
      res.end([
        { type: 'result', is_main_result: true, text: JSON.stringify({ count: values.length, total, minimum: Math.min(...values), maximum: Math.max(...values) }) },
        { type: 'end_of_execution' },
      ].map(item => JSON.stringify(item)).join('\n'));
      return true;
    }
    return false;
  };
  return fixture;
}

export async function testCubeQuantity({ api, db, token, actor, fixture, stop, start }) {
  const conversation = await api('/conversations', { token, method: 'POST', body: { request_id: randomUUID() }, status: 201 });
  const input = await api(`/cases/${conversation.id}/conversation/messages`, { token, method: 'POST', body: { client_message_id: randomUUID(), text: 'Synthetic quantity adapter test' }, status: 201 });
  const session = (await api(`/cases/${conversation.id}/conversation`, { token })).session_id;
  const claim = await api(`/agent-sessions/${session}/claim`, { token, method: 'POST', body: {} });
  await db.query("INSERT INTO agent_runs(message_id,tenant_id,case_id,state) VALUES($1,$2,$3,'running')", [input.message.id, actor.tenantId, conversation.id]);
  const path = `/internal/agent-sessions/${session}/cube/quantity-summary`;
  const body = { tool_call_id: 'quantity_success', quantities: [12, 18, 20] };
  const request = (value = body, status = 200) => api(path, { token: claim.storage_token, method: 'POST', body: value, status });
  const row = async id => (await db.query('SELECT state,result,error_code,cleanup_confirmed FROM cube_quantity_tool_calls WHERE session_id=$1 AND tool_call_id=$2', [session, id])).rows[0];

  await api(path, { token: claim.storage_token, method: 'POST', body: { ...body, code: 'untrusted' }, status: 422, raw: true });
  assert.equal(fixture.creates, 0);
  const concurrent = await Promise.all([request(body, [200, 409]), request(body, [200, 409])]);
  const result = concurrent.find(item => item.total === 50);
  assert(result);
  assert.equal(result.cleanup_confirmed, true);
  assert.equal(fixture.creates, 1);
  assert.equal(fixture.executions, 1);
  assert.equal(fixture.instances.size, 0);
  await stop(); await start();
  assert.deepEqual(await request(), result);
  assert.equal((await request({ ...body, quantities: [1] }, 409)).error, 'cube_tool_call_mismatch');
  assert.equal(fixture.creates, 1);
  assert.equal((await db.query("SELECT count(*)::int AS n FROM audit_events WHERE case_id=$1 AND event_type='agent.cube_tool_completed'", [conversation.id])).rows[0].n, 1);

  fixture.mode = 'wrong-result';
  assert.equal((await request({ ...body, tool_call_id: 'quantity_wrong' }, 503)).error, 'cube_tool_result_mismatch');
  assert.deepEqual(await row('quantity_wrong'), { state: 'failed', result: null, error_code: 'cube_tool_result_mismatch', cleanup_confirmed: true });
  assert.equal(fixture.instances.size, 0);
  assert.equal((await request({ ...body, tool_call_id: 'quantity_wrong' }, 409)).error, 'cube_tool_failed');

  fixture.mode = 'lost-create';
  assert.equal((await request({ ...body, tool_call_id: 'quantity_lost' })).total, 50);
  assert.equal(fixture.creates, 3);
  assert.equal(fixture.instances.size, 0);

  fixture.mode = 'ok'; fixture.blockDelete = true;
  assert.equal((await request({ ...body, tool_call_id: 'quantity_delete' }, 503)).error, 'cube_tool_cleanup_failed');
  assert.equal((await row('quantity_delete')).cleanup_confirmed, false);
  assert.equal(fixture.instances.size, 1);
  fixture.blockDelete = false;

  fixture.mode = 'late-create';
  assert.equal((await request({ ...body, tool_call_id: 'quantity_late' }, 503)).error, 'cube_tool_cleanup_unconfirmed');
  assert.equal((await row('quantity_late')).cleanup_confirmed, false);
  assert.equal(fixture.instances.size, 2);
  const createsBeforeSweep = fixture.creates;
  const executionsBeforeSweep = fixture.executions;
  assert.equal((await request({ ...body, tool_call_id: 'quantity_late' }, 409)).error, 'cube_tool_failed');
  fixture.mode = 'ok';
  await db.query("UPDATE cube_quantity_tool_calls SET created_at=now()-interval '2 minutes' WHERE session_id=$1 AND NOT cleanup_confirmed", [session]);
  await stop(); await start();
  const deadline = Date.now() + 40_000;
  while (Date.now() < deadline && !(await row('quantity_late')).cleanup_confirmed) await sleep(250);
  for (const id of ['quantity_late', 'quantity_delete']) {
    const reconciled = await row(id);
    assert.equal(reconciled.state, 'failed');
    assert.equal(reconciled.cleanup_confirmed, true);
  }
  assert.equal(fixture.instances.size, 0);
  assert.equal(fixture.creates, createsBeforeSweep);
  assert.equal(fixture.executions, executionsBeforeSweep);
  await db.query("UPDATE agent_runs SET state='completed',finished_at=now() WHERE message_id=$1", [input.message.id]);
  await request({ ...body, tool_call_id: 'after_run' }, 403);
  assert.equal(fixture.creates, createsBeforeSweep);
}
