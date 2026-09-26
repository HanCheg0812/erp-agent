import { useEffect, useId, useLayoutEffect, useRef, useState } from 'react';
import { useStreamingText } from '@astryxdesign/core/hooks';
import { BookOpen, Brain, Check, ChevronDown, ClipboardCheck, Database, FileText, ListChecks, LoaderCircle, Monitor, RotateCcw, Search, X } from 'lucide-react';
import Markdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import type { AgentResponse, AgentStep, CompactionEvent, ProposalInput, ToolPermission } from '../../../packages/contracts/src/index';
import { api } from './api';

const toolLabels: Record<string, { label: string; Icon: typeof FileText }> = {
  memory_save: { label: '保存个人偏好', Icon: Brain },
  case_set_context: { label: '更新案件信息', Icon: FileText },
  proposal_create: { label: '生成变更方案', Icon: FileText },
  request_proposal_approval: { label: '提交业务审批', Icon: ClipboardCheck },
  case_read: { label: '查看案件资料', Icon: Database },
  todo: { label: '更新分析进度', Icon: ListChecks },
  skill_find: { label: '查找业务指引', Icon: Search },
  skill_manage: { label: '读取业务指引', Icon: BookOpen },
  attachment_read: { label: '查看附件', Icon: FileText },
  cube_quantity_summary: { label: '汇总数量', Icon: Database },
  bash: { label: '读取运行环境', Icon: Monitor },
  mcp__erp__list_erp_orders: { label: '查看订单', Icon: Database },
  mcp__erp__read_erp_order: { label: '查看订单详情', Icon: Database },
  mcp__openviking__search_case_knowledge: { label: '搜索业务资料', Icon: Search },
  mcp__openviking__read_knowledge_source: { label: '打开资料来源', Icon: BookOpen },
};

const toolStateLabels: Record<string, string> = {
  running: '进行中',
  completed: '已完成',
  failed: '失败',
};

function AgentMarkdown({ text }: { text: string }) {
  return <div className="agent-markdown"><Markdown skipHtml remarkPlugins={[remarkGfm]} components={{ a: ({ children, href }) => <a href={href} target="_blank" rel="noopener noreferrer">{children}</a>, img: ({ alt }) => <span>{alt}</span>, table: ({ children }) => <div className="agent-table-scroll"><table>{children}</table></div> }}>{text}</Markdown></div>;
}

function StreamingText({ text, active, reasoning = false }: { text: string; active: boolean; reasoning?: boolean }) {
  const [revealing, setRevealing] = useState(active);
  const displayed = useStreamingText(text, active || revealing, { speed: 'natural' });
  useEffect(() => {
    if (active) setRevealing(true);
    else if (displayed === text) setRevealing(false);
  }, [active, displayed, text]);
  const typing = active || displayed !== text;
  return <div className={`streaming-text ${reasoning ? 'reasoning-text' : 'answer-text'}`} data-streaming={typing || undefined} aria-busy={typing}>
    <AgentMarkdown text={displayed} />
    {typing && !!text && <span className="typing-cursor" aria-hidden="true" />}
  </div>;
}

function ReasoningStep({ step, expanded, active }: { step: Extract<AgentStep, { kind: 'reasoning' }>; expanded: boolean; active: boolean }) {
  const viewport = useRef<HTMLDivElement>(null);
  const follow = useRef(true);
  useLayoutEffect(() => {
    if (expanded && active && step.state === 'running' && follow.current && viewport.current) {
      viewport.current.scrollTop = viewport.current.scrollHeight;
    }
  }, [expanded, active, step.state, step.text]);
  return <li className="reasoning-step"><span className="process-marker"><Brain size={13} /></span><div className="process-step-content"><div className="reasoning-text" ref={viewport} role="region" aria-label="Reasoning step" tabIndex={expanded ? 0 : -1} onScroll={event => {
    const node = event.currentTarget;
    follow.current = node.scrollHeight - node.scrollTop - node.clientHeight < 32;
  }}><AgentMarkdown text={step.text} /></div>{step.state === 'failed' && <small>Interrupted</small>}</div></li>;
}

// Follows Astryx ChatReasoning's compact, expandable layout; the timeline also
// places actual tool calls between the provider's successive reasoning segments.
function ThinkingProcess({ response }: { response: AgentResponse }) {
  const active = response.state === 'queued' || response.state === 'running';
  const contentId = useId();
  const steps: AgentStep[] = response.steps?.length ? response.steps : response.tools.map(tool => ({ kind: 'tool', id: tool.id }));
  const hasReasoning = steps.some(step => step.kind === 'reasoning');
  const [expanded, setExpanded] = useState(hasReasoning);
  const expandedReasoning = useRef(hasReasoning);
  useEffect(() => {
    // A live response can receive its first reasoning delta after the shell
    // has rendered. Open it once so the user can see that effort is active;
    // subsequent clicks still control the collapsed state.
    if (hasReasoning && !expandedReasoning.current) {
      expandedReasoning.current = true;
      setExpanded(true);
    }
  }, [hasReasoning]);
  const thinking = steps.some(step => step.kind === 'reasoning' && step.state === 'running');
  const label = response.state === 'queued' ? 'Queued' : active ? thinking || !steps.length ? 'Thinking' : 'Working' : response.state === 'failed' ? 'Process interrupted' : steps.some(step => step.kind === 'reasoning') ? 'Thought process' : 'Tool activity';
  if (!steps.length && !active) return null;
  return <section className="thinking-process" aria-label="Thinking process">
    <button className="thinking-toggle" type="button" aria-expanded={expanded} aria-controls={contentId} onClick={() => setExpanded(!expanded)} disabled={!steps.length}>
      {active ? <LoaderCircle size={14} className="spinning" /> : response.state === 'failed' ? <X size={14} /> : <Brain size={14} />}
      <span className={active ? 'thinking-label active' : 'thinking-label'}>{label}</span>
      {!!steps.length && <><span className="thinking-count">{steps.length} {steps.length === 1 ? 'step' : 'steps'}</span><ChevronDown size={13} className="thinking-chevron" /></>}
    </button>
    {/* Keep children mounted like Astryx ChatReasoning, including their scroll position. */}
    <div className="thinking-collapse" data-expanded={expanded} inert={!expanded} id={contentId}><div className="thinking-clip"><div className="thinking-content"><ol className="process-timeline">{steps.map(step => {
      if (step.kind === 'reasoning') return <ReasoningStep key={step.id} step={step} expanded={expanded} active={active} />;
      const tool = response.tools.find(tool => tool.id === step.id);
      if (!tool) return null;
      const { label, Icon } = toolLabels[tool.name] ?? { label: '执行操作', Icon: FileText };
      return <li className="process-tool" key={step.id}><span className="process-marker">{tool.state === 'running' ? <LoaderCircle size={13} className="spinning" /> : tool.state === 'failed' ? <X size={13} /> : <Check size={13} />}</span><div className="process-step-content"><span className="process-tool-name"><Icon size={13} />{label}<small>{toolStateLabels[tool.state] ?? '已处理'}</small></span></div></li>;
    })}</ol></div></div></div>
  </section>;
}

function CompactionPanel({ event }: { event: CompactionEvent }) {
  const before = event.pre_tokens == null ? 'Unknown' : event.pre_tokens.toLocaleString();
  const after = event.post_tokens == null ? 'Unknown' : event.post_tokens.toLocaleString();
  return <details className="compaction-panel">
    <summary><span className="compaction-mark"><RotateCcw size={13} /></span><strong>Context compressed</strong><span>{event.trigger.replaceAll('_', ' ')}</span><ChevronDown size={13} /></summary>
    <div className="compaction-details"><dl><div><dt>Before (estimated)</dt><dd>{before} tokens</dd></div><div><dt>After (estimated)</dt><dd>{after} tokens</dd></div><div><dt>Messages kept</dt><dd>{event.active_messages ?? 'Unknown'}</dd></div><div><dt>Summary</dt><dd>{event.summary_chars == null ? 'Not recorded' : `${event.summary_chars.toLocaleString()} chars`}</dd></div><div><dt>Tokens saved</dt><dd>{event.tokens_saved?.toLocaleString() ?? 'Unknown'}</dd></div><div><dt>Duration</dt><dd>{event.duration_ms == null ? 'Unknown' : `${(event.duration_ms / 1000).toFixed(1)} s`}</dd></div></dl></div>
  </details>;
}

function ProposalChanges({ proposal }: { proposal: Pick<ProposalInput, 'summary' | 'actions' | 'evidence'> }) {
  return <div className="permission-changes"><p>{proposal.summary}</p>{proposal.actions.map((action, index) => <div className="permission-action" key={index}>
    <strong>{action.sales_order} / {action.item_row}</strong>
    <dl><div><dt>数量</dt><dd>{action.expected_quantity.toLocaleString()} → <strong>{action.proposed_quantity.toLocaleString()}</strong></dd></div><div><dt>交期</dt><dd>{action.expected_delivery_date} → <strong>{action.proposed_delivery_date}</strong></dd></div></dl>
  </div>)}<details><summary>{proposal.evidence.length} 条证据</summary>{proposal.evidence.map((source, index) => <p key={index}><code>{source.document_id}</code><br />{source.revision} · {source.locator}</p>)}</details></div>;
}

function PermissionCard({ permission, caseId, enabled, onDecision }: { permission: ToolPermission; caseId: string; enabled: boolean; onDecision?: () => void }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const pending = permission.state === 'pending';
  const business = permission.tool === 'request_proposal_approval';
  const { label, Icon } = toolLabels[permission.tool] ?? { label: '执行操作', Icon: FileText };
  const title = pending ? business ? '等待业务审批' : permission.mode === 'auto' ? '正在审核操作' : '请求批准' : permission.state === 'executed' ? business ? permission.result?.status === 'approved' ? '提案已批准' : '提案已拒绝' : '操作已完成' : permission.state === 'allowed' ? '已批准，等待执行' : permission.state === 'cancelled' ? '操作已取消' : '操作未获批准';
  const decide = async (decision: 'allowed' | 'denied') => {
    setBusy(true); setError('');
    try {
      if (permission.tool === 'request_proposal_approval') {
        await api.decide(caseId, { ...permission.arguments, decision: decision === 'allowed' ? 'approved' : 'rejected', comment: '' }, `permission-${permission.id}`);
      } else await api.decideTool(caseId, permission.id, decision, permission.arguments_digest);
      onDecision?.();
    }
    catch (reason) { setError(reason instanceof Error ? reason.message : '批准失败，请重试'); }
    finally { setBusy(false); }
  };
  return <section className="tool-permission" aria-label={business ? 'Business approval' : 'Tool approval'}>
    <header>{pending && permission.mode === 'auto' ? <LoaderCircle size={16} className="spinning" /> : <Icon size={16} />}<strong>{title}</strong><span>{label}</span></header>
    {permission.tool === 'memory_save' && <p>{permission.arguments.text}</p>}
    {permission.tool === 'case_set_context' && <div className="permission-changes"><p>{permission.arguments.context.title}</p><dl>{Object.entries({ 客户: permission.arguments.context.customer_code, 料号: permission.arguments.context.part_number, 版本: permission.arguments.context.revision, 需求: { forecast: '预测', order: '客户订单', delivery_instruction: '交货指令' }[permission.arguments.context.demand_type], 数量口径: { replacement: '替换总量', incremental: '增量', cumulative: '累计', unknown: '待确认' }[permission.arguments.context.quantity_basis] }).map(([label, value]) => <div key={label}><dt>{label}</dt><dd>{value}</dd></div>)}</dl></div>}
    {permission.tool === 'proposal_create' && <ProposalChanges proposal={permission.arguments} />}
    {permission.tool === 'request_proposal_approval' && <><p className="permission-version">提案 v{permission.proposal.case_version} · <code>{permission.proposal.digest.slice(0, 12)}</code></p><ProposalChanges proposal={permission.proposal} /></>}
    <footer><span>{business ? '审批仅记录决定，ERP 尚未执行' : permission.decided_by === 'reviewer' ? '独立审核' : permission.mode === 'full' ? '完全访问' : '当前账户 / 当前案件'}{permission.reason ? ` · ${permission.reason}` : ''}</span>{pending && permission.mode === 'ask' && enabled && (!business || permission.can_approve) && <div><button type="button" className="button" disabled={busy} onClick={() => void decide('denied')}><X size={14} />{business ? '拒绝提案' : '拒绝'}</button><button type="button" className="button primary-button" disabled={busy} onClick={() => void decide('allowed')}><Check size={14} />{business ? '批准提案' : '批准'}</button></div>}</footer>
    {pending && business && !permission.can_approve && <p className="response-stopped">当前账户没有业务审批权限</p>}
    {error && <p className="conversation-error" role="alert">{error}</p>}
  </section>;
}

export function AgentReply({ response, compacting = false, caseId, canDecide = false, onDecision }: { response: AgentResponse; compacting?: boolean; caseId?: string; canDecide?: boolean; onDecision?: () => void }) {
  const active = response.state === 'queued' || response.state === 'running';
  return <>
    {compacting ? active && <div className="compaction-pending" role="status"><LoaderCircle size={14} className="spinning" />Compressing context...</div> : <ThinkingProcess response={response} />}
    {response.compactions?.map((event, index) => <CompactionPanel key={`${event.trigger}-${index}`} event={event} />)}
    <StreamingText text={response.text} active={active} />
    {caseId && response.permissions?.map(permission => <PermissionCard key={permission.id} permission={permission} caseId={caseId} enabled={canDecide && ['running','waiting'].includes(response.state)} onDecision={onDecision} />)}
    {response.state === 'failed' && <p className="conversation-error" role="alert">{response.error}</p>}
    {response.state === 'cancelled' && <p className="response-stopped">Stopped</p>}
  </>;
}
