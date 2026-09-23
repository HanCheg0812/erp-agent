import { useEffect, useState, type ReactNode } from 'react';
import { ArrowRight, BookOpen, Bot, Check, ClipboardCheck, Database, FileText, LockKeyhole, Pause, Play, RotateCcw, type LucideIcon } from 'lucide-react';
import type { CaseDetail } from './api';
import { agentStages, demoAgentProposal, type AgentRun } from './demo-agent';

type NodeId = 'agent' | 'erp' | 'knowledge' | 'proposal' | 'approval' | 'execution';
type NodeState = 'queued' | 'active' | 'done' | 'waiting' | 'blocked';

export function AgentFlow({ detail, run, onRun, onPause, demoMode, onReview, onEvidence }: {
  detail: CaseDetail; run?: AgentRun; onRun: () => void; onPause: () => void;
  demoMode: boolean; onReview: () => void; onEvidence: () => void;
}) {
  const [selected, setSelected] = useState<NodeId>(run?.status === 'complete' ? 'knowledge' : 'agent');
  useEffect(() => {
    if (run?.status === 'running') setSelected(agentStages[run.activeStage].node);
  }, [run?.activeStage, run?.status]);

  if (!demoMode) return <section className="empty-state"><Bot size={24} /><h3>No activity yet</h3>{detail.proposals.length > 0 && <button className="button secondary-button" onClick={onReview}><FileText size={14} />Review proposal</button>}</section>;

  const fixture = demoAgentProposal(detail);
  if (!fixture || !run) return <section className="empty-state"><Bot size={24} /><h3>No simulation for this case</h3><p>No ERP or knowledge fixture is attached.</p></section>;

  const state = run;
  const finishing = state.status === 'complete' && !state.finalized;
  const complete = state.status === 'complete' && !finishing;
  const running = state.status === 'running';
  const paused = state.status === 'paused';
  const action = fixture.actions[0];
  const evidence = fixture.evidence[0];
  const nodeState = (start: number, end = start): NodeState => complete ? 'done'
    : state.status === 'idle' ? 'queued' : state.activeStage > end ? 'done'
      : state.activeStage >= start ? 'active' : 'queued';
  const resultReady = (stage: number) => complete || state.status !== 'idle' && state.activeStage > stage;
  const rejected = detail.status === 'rejected';
  const approvalState = !complete ? 'queued' : detail.status === 'approved' ? 'done' : rejected ? 'blocked' : 'waiting';
  const status = finishing ? 'Preparing the approval proposal' : running ? agentStages[state.activeStage].label : paused ? `Paused: ${agentStages[state.activeStage].label}`
    : complete ? detail.status === 'approved' ? 'Approval recorded / ERP execution pending' : rejected ? 'Proposal rejected / revision required' : 'Analysis complete / awaiting approval' : 'Ready to analyze';
  const node = (id: NodeId, title: string, subtitle: string, icon: LucideIcon, nodeStatus: NodeState, label?: string) =>
    <FlowNode id={id} title={title} subtitle={subtitle} icon={icon} state={nodeStatus} selected={selected === id} onSelect={setSelected} label={label ?? (nodeStatus === 'active' && paused ? 'Paused' : undefined)} />;

  return <section className="agent-flow">
    <div className="flow-toolbar"><div><span className="section-kicker">SIMULATION / NO EXTERNAL CALLS</span><h3>Agent flow</h3></div><button className="button secondary-button" disabled={finishing} onClick={running ? onPause : onRun}>{running ? <Pause size={14} /> : complete ? <RotateCcw size={14} /> : <Play size={14} />}{finishing ? 'Finishing' : running ? 'Pause' : paused ? 'Resume' : complete ? 'Replay' : 'Run demo'}</button></div>
    <div className={`flow-status ${running ? 'running' : ''}`} role="status"><span />{status}</div>
    <div className={`flow-graph ${running ? 'flow-running' : ''}`} aria-label="Agent tool calls and approval flow">
      <div className="flow-center">{node('agent', 'JCode Agent', 'Demand change analysis', Bot, nodeState(0, 5))}</div>
      <div className="flow-connector branch" aria-hidden="true" />
      <div className="flow-pair tool-pair">
        {node('erp', 'ERPNext', 'Read demand baseline', Database, nodeState(1))}
        {node('knowledge', 'Knowledge', 'Retrieve scoped evidence', BookOpen, nodeState(3))}
      </div>
      <div className="flow-connector merge" aria-hidden="true" />
      <div className="flow-center">{node('proposal', 'Change proposal', 'Evaluate impact + draft', FileText, nodeState(4, 5))}</div>
      <div className="flow-connector approval-connector" aria-hidden="true" />
      <div className="flow-pair gate-pair">
        {node('approval', 'Human approval', complete && (detail.status === 'approved' || rejected) ? 'Decision recorded' : 'Exact proposal version', ClipboardCheck, approvalState, complete && rejected ? 'Rejected' : approvalState === 'waiting' ? 'Awaiting review' : undefined)}
        {node('execution', 'ERP execution', 'Requires approved actions', LockKeyhole, 'blocked', 'Not sent')}
      </div>
    </div>
    <section className="flow-inspector" aria-label="Selected flow node">
      <div className="inspector-heading"><h4>{selected === 'knowledge' ? 'Knowledge' : selected === 'erp' ? 'ERPNext' : selected === 'agent' ? 'JCode Agent' : selected === 'proposal' ? 'Change proposal' : selected === 'approval' ? 'Human approval' : 'ERP execution'}</h4><span>{selected === 'execution' ? 'NO WRITE' : 'SIMULATED'}</span></div>
      {selected === 'agent' && <><p className="inspector-copy">Analyze the customer request against the ERP baseline and the applicable document revision.</p><dl className="inspector-fields"><Field label="Customer" value={detail.customer_code ?? 'Not supplied'} /><Field label="Part / revision" value={detail.part_number ? `${detail.part_number} / ${detail.revision ?? 'Unspecified'}` : 'Not supplied'} /><Field label="Demand" value={detail.demand_type?.replaceAll('_', ' ') ?? 'Not supplied'} /><Field label="Quantity basis" value={detail.quantity_basis} /></dl></>}
      {selected === 'erp' && <><code className="tool-call">get_demand_baseline</code>{resultReady(1) ? <dl className="inspector-fields"><Field label="Sales order / row" value={`${action.sales_order} / ${action.item_row}`} /><Field label="Current quantity" value={`${action.expected_quantity.toLocaleString()} units`} /><Field label="Delivery date" value={action.expected_delivery_date} /><Field label="ERP modified" value={action.expected_erp_modified} /></dl> : <p className="inspector-copy">{nodeState(1) === 'active' ? 'Reading synthetic ERP baseline...' : 'No tool result yet.'}</p>}</>}
      {selected === 'knowledge' && <><code className="tool-call">search_case_knowledge</code><p className="knowledge-scope">{detail.customer_code ?? 'No customer'} / {detail.part_number ?? 'No part'} / Rev {detail.revision ?? 'Unspecified'}</p>{resultReady(3) ? <><div className="source-heading"><FileText size={15} /><strong>Customer delivery instruction</strong><span>{evidence.revision}</span></div><blockquote className="source-excerpt">Replace the order total with {action.proposed_quantity.toLocaleString()} units. Requested delivery: {action.proposed_delivery_date}.</blockquote><div className="source-locator">{evidence.locator}</div><code className="source-uri">viking://resources/demo/{(detail.customer_code ?? 'unknown').toLowerCase()}/{(detail.part_number ?? 'unknown').toLowerCase()}/delivery-instruction/{evidence.revision.toLowerCase()}</code>{detail.proposals.length > 0 && <button className="text-button" onClick={onEvidence}>Evidence references<ArrowRight size={13} /></button>}</> : <p className="inspector-copy">{nodeState(3) === 'active' ? 'Retrieving synthetic evidence in the case scope...' : 'No tool result yet.'}</p>}</>}
      {selected === 'proposal' && <>{resultReady(5) ? <><p className="inspector-copy">{fixture.summary}</p><dl className="inspector-fields"><Field label="Quantity" value={`${action.expected_quantity.toLocaleString()} → ${action.proposed_quantity.toLocaleString()}`} /><Field label="Delivery" value={`${action.expected_delivery_date} → ${action.proposed_delivery_date}`} /></dl></> : <p className="inspector-copy">{nodeState(4, 5) === 'active' ? 'Comparing the baseline with the controlled instruction...' : 'Waiting for ERP and knowledge results.'}</p>}</>}
      {selected === 'approval' && <><p className="inspector-copy">{!complete ? 'Waiting for the analysis result.' : detail.status === 'approved' ? 'Decision recorded. ERP execution has not been performed.' : rejected ? 'Proposal rejected. A revised proposal is required before another decision.' : 'Waiting for an approver to review the proposal and evidence.'}</p>{complete && <dl className="inspector-fields"><Field label="Proposal version" value={`v${fixture.case_version}`} /><Field label="Digest" value={fixture.digest.slice(0, 12)} /></dl>}</>}
      {selected === 'execution' && <><p className="inspector-copy">No ERP write has been sent.</p><dl className="inspector-fields"><Field label="Approval" value={detail.status === 'approved' ? 'Recorded' : rejected ? 'Rejected' : 'Pending'} /><Field label="ERP revalidation" value="Not performed" /><Field label="Execution receipt" value="None" /></dl></>}
    </section>
    <div className="flow-footer"><span>{complete ? `${fixture.evidence.length} evidence references / ${fixture.actions.length} proposed action` : `Step ${state.status === 'idle' ? 0 : state.activeStage + 1} of ${agentStages.length}`}</span>{complete && detail.proposals.length > 0 && <button className="button primary-button" onClick={onReview}><ClipboardCheck size={14} />Review proposal<ArrowRight size={14} /></button>}</div>
  </section>;
}

function FlowNode({ id, title, subtitle, icon: Icon, state, selected, onSelect, label }: {
  id: NodeId; title: string; subtitle: string; icon: LucideIcon; state: NodeState; selected: boolean; onSelect: (id: NodeId) => void; label?: string;
}) {
  return <button className={`flow-node ${state} ${selected ? 'inspected' : ''}`} onClick={() => onSelect(id)} aria-pressed={selected} aria-label={`${title}: ${label ?? state}`}><span className="flow-node-top"><Icon size={16} /><strong>{title}</strong>{state === 'done' && <Check className="node-check" size={13} />}</span><span className="flow-node-subtitle">{subtitle}</span><span className="flow-node-state">{label ?? ({ queued: 'Queued', active: 'Running', done: 'Complete', waiting: 'Waiting', blocked: 'Blocked' }[state])}</span></button>;
}

function Field({ label, value }: { label: string; value: ReactNode }) {
  return <div><dt>{label}</dt><dd>{value}</dd></div>;
}
