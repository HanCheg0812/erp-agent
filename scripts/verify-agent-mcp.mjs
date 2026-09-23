import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { mkdir, writeFile } from 'node:fs/promises';
import pg from 'pg';

const origin = process.env.VERIFY_API_ORIGIN ?? 'http://127.0.0.1:3100';
const output = process.env.VERIFY_MCP_OUTPUT ?? '/tmp/erp-mcp-acceptance';
let token;
async function api(path, body) {
  const r = await fetch(`${origin}/v1${path}`, {method:body ? 'POST':'GET',headers:{'content-type':'application/json',...(token ? {authorization:`Bearer ${token}`} : {})},...(body ? {body:JSON.stringify(body)} : {})});
  const data=await r.json(); assert(r.ok,`${r.status}: ${JSON.stringify(data)}`); return data;
}
token=(await api('/auth/login',{email:process.env.DEMO_EMAIL,password:process.env.DEMO_PASSWORD})).access_token;
const record=await api('/conversations',{request_id:randomUUID()});
const started=Date.now();
const message=await api(`/cases/${record.id}/conversation/messages`,{client_message_id:randomUUID(),settings:{approval:'ask',model:'deepseek-flash',effort:'low'},text:'请先用 ERP 工具列出当前有权限的订单，再读取列表中第一张订单，简要给出真实订单号、客户、物料及数量。然后调用 OpenViking 搜索相关交付资料；如果未配置，直接说明工具返回的原因。不修改数据。'});
let history;
let response;
for(let i=0;i<175;i++) {
  history=await api(`/cases/${record.id}/conversation`);
  response=history.items.find(item=>item.id===message.message.id)?.response;
  if(response && ['completed','failed','cancelled'].includes(response.state)) break;
  await new Promise(resolve=>setTimeout(resolve,1000));
}
await mkdir(output,{recursive:true});
const report={case_id:record.id,session_id:history.session_id,message_id:message.message.id,duration_ms:Date.now()-started,config:history.config,response};
await writeFile(`${output}/report.json`,JSON.stringify(report,null,2));
console.log(JSON.stringify({case_id:record.id,state:response?.state,duration_ms:report.duration_ms,tools:response?.tools,report:`${output}/report.json`},null,2));
assert.equal(response?.state,'completed');
for(const tool of ['mcp__erp__list_erp_orders','mcp__erp__read_erp_order']) assert(response.tools.some(t=>t.name===tool&&t.state==='completed'),`Missing successful ${tool}`);
assert(response.tools.some(t=>t.name==='mcp__openviking__search_case_knowledge'&&t.state==='failed'),'Unconfigured knowledge service must return a tool error');
assert(response.text.includes('SAL-ORD-'));
const db=new pg.Client({connectionString:process.env.DATABASE_URL}); await db.connect();
try {
  const saved=(await db.query('SELECT metadata FROM agent_sessions WHERE id=$1',[history.session_id])).rows[0].metadata;
  assert.equal(saved.reasoning_effort,'low');
  console.log(JSON.stringify({persisted_model:saved.model,persisted_effort:saved.reasoning_effort}));
} finally {await db.end();}
