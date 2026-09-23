import { createHash } from 'node:crypto';
import { setTimeout } from 'node:timers/promises';

let raw = '';
for await (const chunk of process.stdin) raw += chunk;
const input = JSON.parse(raw);
if (!input.permission_context || typeof input.permission_context !== 'object') throw Error('Runtime input is missing permission_context');
if (input.permission_context.mode !== input.approval) throw Error('Runtime permission mode does not match approval setting');
for (const tool of ['memory_save', 'case_set_context', 'proposal_create']) {
  if (!input.permission_context.operation_tools?.includes(tool)) throw Error(`Runtime permission context is missing ${tool}`);
}
if (input.permission_context.business_approval_tool !== 'request_proposal_approval') throw Error('Runtime permission context is missing proposal approval tool');
const emit = event => process.stdout.write(`${JSON.stringify(event)}\n`);
const digest = text => createHash('sha256').update(text).digest('hex');
const source = 'Synthetic test guidance';
const skill = { id: 'test-review', version: '1.0.0', sha256: digest(source) };
const snapshot = JSON.stringify({ schema_version: 1, id: 'test@1', skills: [{ ...skill, source }] });
emit({ type: 'skill_catalog', snapshot, hash: digest(snapshot) });
if (input.operation === 'compact') {
  emit({ type: 'compaction_started' });
  if (input.case.part_number === 'FAIL-COMPACTION') {
    emit({ type: 'failed' });
    process.exitCode = 1;
  } else {
    emit({ type: 'compaction', trigger: 'manual', pre_tokens: 12000, post_tokens: input.case.part_number === 'INVALID-COMPACTION' ? -1 : 3000, tokens_saved: 9000, duration_ms: 500, messages_dropped: null, messages_compacted: 20, summary_chars: 600, active_messages: 10 });
    emit({ type: 'completed' });
  }
} else if (input.text === 'checkpoint shutdown') {
  process.once('SIGTERM', async () => {
    const endpoint = `${process.env.JCODE_SESSION_STORE_URL}/internal/agent-sessions/${process.env.JCODE_SESSION_STORE_ID}/documents/stream_checkpoint`;
    const headers = { authorization: `Bearer ${process.env.JCODE_SESSION_STORE_TOKEN}`, 'content-type': 'application/json' };
    const current = await fetch(endpoint, { headers }).then(r => r.json());
    const saved = await fetch(endpoint, { method: 'PUT', headers, body: JSON.stringify({ request_id: crypto.randomUUID(), expected_revision: current.revision, payload: { text: 'Saved during shutdown', after_message_id: 'fixture' } }) });
    if (!saved.ok) throw Error(`Shutdown checkpoint rejected: ${saved.status}`);
    emit({ type: 'failed', code: 'interrupted' });
    process.exit(0);
  });
  emit({ type: 'text_delta', text: 'Saved during shutdown' });
  emit({ type: 'text_done' });
  await setTimeout(60000);
} else if (input.text.startsWith('memory:') || input.text.startsWith('action:')) {
  const call = input.text.startsWith('memory:') ? {tool:'memory_save',arguments:{text:input.text.slice(7)}} : JSON.parse(input.text.slice(7));
  const id=`action-${digest(input.text).slice(0,20)}`;
  emit({type:'tool_start',id,name:call.tool});
  const endpoint = `${process.env.JCODE_SESSION_STORE_URL}/internal/agent-sessions/${process.env.JCODE_SESSION_STORE_ID}`;
  const headers = {authorization:`Bearer ${process.env.JCODE_SESSION_STORE_TOKEN}`,'content-type':'application/json'};
  const options = {method:'POST',headers,body:JSON.stringify({tool_call_id:id,tool:call.tool,arguments:call.arguments})};
  const response=await fetch(`${endpoint}/actions`,options);
  if(!response.ok)throw Error(`Action status ${response.status}: ${await response.text()}`);
  const action=await response.json();
  if(action.status==='pending')emit({type:'suspended'});
  else {
    const replay = await fetch(`${endpoint}/actions`,options).then(r=>r.json());
    if(JSON.stringify(replay)!==JSON.stringify(action))throw Error('Tool replay changed its result');
    if(call.tool==='case_set_context' && action.status==='saved') {
      const fresh = await fetch(`${endpoint}/case`,{headers}).then(r=>r.json());
      if(fresh.version!==action.case.version || fresh.kind!=='case')throw Error('case_read is stale');
      if(fresh.authorization?.mode !== input.approval)throw Error('case_read authorization mode is stale');
      if(!fresh.authorization?.read_tools?.includes('case_read'))throw Error('case_read authorization is missing read tools');
    }
    emit({type:'tool_done',id,name:call.tool,failed:false});emit({type:'text_delta',text:action.status});emit({type:'completed'});
  }
} else if (input.text === 'unknown tool') {
  emit({ type: 'tool_start', id: 'bad', name: 'bash' });
} else if (input.text === 'wrong skill') {
  emit({ type: 'skill_loaded', ...skill, version: '2.0.0' });
} else {
  emit({ type: 'skill_loaded', ...skill });
  emit({ type: 'reasoning_delta', text: 'Checking saved facts.' });
  emit({ type: 'tool_start', id: 'read-1', name: 'case_read' });
  if (input.text === 'interrupted') {
    emit({ type: 'text_delta', text: 'Saved partial reply' });
    emit({ type: 'reasoning_delta', text: 'Pending check' });
    process.exitCode = 1;
  } else {
    await setTimeout(input.text === 'long' ? 151000 : input.text === 'slow' ? 30000 : input.text.startsWith('parallel:') ? 2000 : 800);
    emit({ type: 'tool_done', id: 'read-1', name: 'case_read', failed: false });
    emit({ type: 'text_delta', text: `Scoped reply for ${input.case.id}` });
    emit({ type: 'completed' });
  }
}
