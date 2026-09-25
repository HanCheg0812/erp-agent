import assert from 'node:assert/strict';
import pg from 'pg';
import { randomBytes, randomUUID, scryptSync } from 'node:crypto';

const base = process.env.VALIDATION_API_ORIGIN || 'http://127.0.0.1:3100';
const limitedEmail = 'erp-boundary-limited@example.test';
const limitedPassword = process.env.VALIDATION_LIMITED_PASSWORD || randomBytes(24).toString('base64url');
const db = new pg.Client({ connectionString: process.env.DATABASE_URL });
await db.connect();
const runIds = [];
const lockedCases = [];

const tenant = (await db.query("SELECT id FROM tenants WHERE slug='demo'")).rows[0].id;
const salt = randomBytes(16).toString('hex');
const digest = scryptSync(limitedPassword, salt, 64, {
  N: 16384,
  r: 8,
  p: 1,
  maxmem: 64 * 1024 * 1024,
}).toString('hex');
const limited = (
  await db.query(
    `INSERT INTO users(id,tenant_id,email,password_hash,roles)
     VALUES($1,$2,$3,$4,ARRAY['planner'])
     ON CONFLICT(tenant_id,email) DO UPDATE
       SET password_hash=EXCLUDED.password_hash,active=true,roles=ARRAY['planner']
     RETURNING id`,
    [randomUUID(), tenant, limitedEmail, `scrypt:${salt}:${digest}`],
  )
).rows[0].id;
const connection = (
  await db.query("SELECT id FROM erp_connections WHERE tenant_id=$1 AND name='local-synthetic-erp'", [tenant])
).rows[0].id;
await db.query(
  `INSERT INTO erp_identity_bindings(tenant_id,platform_user_id,connection_id,erp_user_id,credential_ref,active)
   VALUES($1,$2,$3,'limited-reader@example.test','env:ERP_CREDENTIAL_LIMITED_READER',true)
   ON CONFLICT(tenant_id,platform_user_id) DO UPDATE
     SET connection_id=EXCLUDED.connection_id,erp_user_id=EXCLUDED.erp_user_id,
         credential_ref=EXCLUDED.credential_ref,active=true`,
  [tenant, limited, connection],
);
await db.query('DELETE FROM integration_grants WHERE tenant_id=$1 AND owner_id=$2', [tenant, limited]);
await db.query(
  `INSERT INTO integration_grants(tenant_id,owner_id,domain,scope,tool_id,action,effect,resource_type)
   VALUES($1,$2,'erp','Temu 渠道客户（演示）','list_erp_orders','read','allow','customer'),
         ($1,$2,'erp','Temu 渠道客户（演示）','read_erp_order','read','allow','customer')`,
  [tenant, limited],
);

async function request(path, options = {}) {
  const response = await fetch(`${base}${path}`, {
    method: options.method || 'GET',
    headers: {
      'content-type': 'application/json',
      ...(options.token ? { authorization: `Bearer ${options.token}` } : {}),
    },
    body: options.body === undefined ? undefined : JSON.stringify(options.body),
  });
  const body = response.status === 204 ? null : await response.json();
  if (options.expect !== undefined && response.status !== options.expect) {
    throw new Error(`${options.method || 'GET'} ${path}: expected ${options.expect}, got ${response.status}: ${JSON.stringify(body)}`);
  }
  return { status: response.status, body };
}

async function login(email, password) {
  return (
    await request('/v1/auth/login', {
      method: 'POST',
      body: { tenant: 'demo', email, password },
      expect: 200,
    })
  ).body.access_token;
}

async function prepareRun(token, label, customer) {
  const created = await request('/v1/cases', {
    method: 'POST',
    token,
    body: {
      title: `Observed ERP boundary ${label}`,
      customer_code: customer,
      part_number: label === 'forge' ? 'SH-04-GREY-XL' : 'SH-04-GREY-L',
      revision: 'A',
      demand_type: 'order',
      quantity_basis: 'unknown',
    },
    expect: 201,
  });
  const caseId = created.body.id;
  const lockKey = `agent_run:${caseId}`;
  await db.query('SELECT pg_advisory_lock(hashtextextended($1,0))', [lockKey]);
  lockedCases.push(lockKey);
  const message = await request(`/v1/cases/${caseId}/conversation/messages`, {
    method: 'POST',
    token,
    body: { client_message_id: randomUUID(), text: `Observed ERP boundary ${label}` },
    expect: 201,
  });
  runIds.push(message.body.message.id);
  const history = await request(`/v1/cases/${caseId}/conversation`, { token, expect: 200 });
  const claim = await request(`/v1/agent-sessions/${history.body.session_id}/claim`, {
    method: 'POST',
    token,
    body: {},
    expect: 200,
  });
  await db.query(
    `INSERT INTO agent_runs(message_id,tenant_id,case_id,state)
     VALUES($1,$2,$3,'running')
     ON CONFLICT(message_id) DO UPDATE SET state='running',cancel_requested=false`,
    [message.body.message.id, tenant, caseId],
  );
  return { caseId, id: history.body.session_id, token: claim.body.storage_token };
}

async function callTool(run, tool, argumentsValue) {
  return request(`/internal/agent-sessions/${run.id}/tools`, {
    method: 'POST',
    token: run.token,
    body: { domain: 'erp', tool, arguments: argumentsValue },
  });
}

try {
const plannerToken = await login('planner@example.test', process.env.DEMO_PASSWORD);
const plannerRun = await prepareRun(plannerToken, 'forge', 'Amazon 渠道客户（演示）');
const plannerList = await callTool(plannerRun, 'list_erp_orders', {});
const limitedToken = await login(limitedEmail, limitedPassword);
const limitedRun = await prepareRun(limitedToken, 'limited', 'Temu 渠道客户（演示）');
const limitedList = await callTool(limitedRun, 'list_erp_orders', {});
const limitedRead = await callTool(limitedRun, 'read_erp_order', { sales_order: 'SAL-ORD-2026-00001' });
const nativeDenied = await callTool(limitedRun, 'read_erp_order', { sales_order: 'SAL-ORD-2026-00002' });
await db.query(
  `INSERT INTO integration_grants(tenant_id,owner_id,domain,scope,tool_id,action,effect,resource_type)
   VALUES($1,$2,'erp','Temu 渠道客户（演示）','list_erp_orders','read','deny','customer')`,
  [tenant, limited],
);
const platformDenied = await callTool(limitedRun, 'list_erp_orders', {});
await db.query('DELETE FROM integration_grants WHERE tenant_id=$1 AND owner_id=$2 AND effect=\'deny\'', [tenant, limited]);
const mismatchRun = await prepareRun(limitedToken, 'scope-mismatch', 'Shopee 渠道客户（演示）');
const scopeDenied = await callTool(mismatchRun, 'list_erp_orders', {});
const audit = (
  await db.query(
    `SELECT payload->>'erp_user' AS erp_user,payload->>'trace_id' AS trace_id
     FROM audit_events WHERE case_id=$1 AND event_type='agent.tool_read'
     ORDER BY created_at DESC LIMIT 3`,
    [limitedRun.caseId],
  )
).rows;

console.log(
  JSON.stringify(
    {
      erpnext: { version: 'v16.35.0', base_url: 'http://127.0.0.1:18080', synthetic_orders: 4 },
      forge: {
        erp_user: 'forge-reader@example.test',
        http: plannerList.status,
        orders: plannerList.body?.data?.orders?.map((order) => order.name),
        count: plannerList.body?.data?.orders?.length,
        trace_id: plannerList.body?.trace_id,
      },
      limited: {
        erp_user: 'limited-reader@example.test',
        list: {
          http: limitedList.status,
          orders: limitedList.body?.data?.orders?.map((order) => order.name),
          count: limitedList.body?.data?.orders?.length,
          source: limitedList.body?.sources?.map((source) => source.name),
          trace_id: limitedList.body?.trace_id,
        },
        read: {
          http: limitedRead.status,
          order: limitedRead.body?.data?.order?.name,
          trace_id: limitedRead.body?.trace_id,
        },
        erp_native_denied: {
          http: nativeDenied.status,
          code: nativeDenied.body?.error,
          category: nativeDenied.body?.agent_guidance?.category,
          retryable: nativeDenied.body?.agent_guidance?.retryable,
          next_step: nativeDenied.body?.agent_guidance?.safe_next_step,
        },
        platform_denied: {
          http: platformDenied.status,
          code: platformDenied.body?.error,
          category: platformDenied.body?.agent_guidance?.category,
        },
        scope_mismatch: { http: scopeDenied.status, code: scopeDenied.body?.error },
        audit,
      },
    },
    null,
    2,
  ),
);
assert.equal(plannerList.status, 200, 'Forge ERP read must succeed');
assert.equal(plannerList.body?.data?.orders?.length, 1, 'Forge ERP scope must contain one order');
assert.equal(limitedList.status, 200, 'Limited ERP list must succeed');
assert.equal(limitedList.body?.data?.orders?.length, 1, 'Limited ERP scope must contain one order');
assert.equal(limitedRead.status, 200, 'Limited ERP order read must succeed');
assert.equal(nativeDenied.status, 403, 'ERPNext must deny the other customer order');
assert.equal(nativeDenied.body?.error, 'erp_forbidden');
assert.equal(platformDenied.status, 403, 'Platform deny must override the grant');
assert.equal(platformDenied.body?.error, 'platform_forbidden');
assert.equal(scopeDenied.status, 403, 'Case scope mismatch must be denied');
assert.equal(scopeDenied.body?.error, 'erp_scope_missing');
assert.ok(audit.length > 0, 'ERP tool reads must leave an audit trail');
} finally {
  try {
    if (runIds.length) {
      await db.query('DELETE FROM agent_runs WHERE message_id=ANY($1::bigint[])', [runIds]);
    }
  } finally {
    for (const lockKey of lockedCases.reverse()) {
      await db.query('SELECT pg_advisory_unlock(hashtextextended($1,0))', [lockKey]);
    }
    await db.end();
  }
}
