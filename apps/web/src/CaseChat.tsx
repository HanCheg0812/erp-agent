import { useEffect, useLayoutEffect, useRef, useState, type FormEvent, type ReactNode } from 'react';
import {
  ArrowDown, ArrowRight, ArrowUp, BookOpen, Bot, Check, ChevronDown, ChevronRight,
  ClipboardCheck, Database, FileText, GitBranch, LoaderCircle, Menu, MessageSquare,
  Minimize2, Paperclip, PanelRight, Pause, Play, RotateCcw, ShieldCheck, X,
} from 'lucide-react';
import { api, type AuditEvent, type CaseDetail, type CaseDocument, type ConversationMessage, type Proposal } from './api';
import { agentStages, demoAgentProposal, type AgentRun } from './demo-agent';
import { ChatContextPanel, type ContextSection } from './ChatContextPanel';
import { useConversation } from './useConversation';
import { AgentReply } from './AgentReply';
import { ComposerControls } from './ComposerControls';
import type { AgentSettings } from '../../../packages/contracts/src/index';
import { attachmentContentType } from './attachmentTypes';

type ChatMessage = { id: string; role: 'user' | 'assistant'; text: string };
export type ChatSession = { draft: string; messages: ChatMessage[] };
const steps = agentStages.map(stage => stage.label);
const number = (value: number) => value.toLocaleString('en');
const date = (value: string) => new Intl.DateTimeFormat('en', { month: 'short', day: 'numeric', timeZone: 'UTC' }).format(new Date(`${value}T00:00:00Z`));

function resizeComposer(node: HTMLTextAreaElement) {
  node.style.maxHeight = `${Math.max(72, Math.min(280, (window.visualViewport?.height ?? window.innerHeight) * 0.35))}px`;
  node.style.height = '0px';
  node.style.height = `${node.scrollHeight}px`;
  node.style.overflowY = node.scrollHeight > node.clientHeight + 1 ? 'auto' : 'hidden';
}

export function CaseChat({ detail, events, run, demoMode, onRun, onPause, onDecide, busy, canApprove, onNavigation, session, onSession, onSent, onAddCaseDetails, onVersionChange }: {
  detail: CaseDetail; events: AuditEvent[]; run?: AgentRun; demoMode: boolean;
  onRun: () => void; onPause: () => void; onDecide: (decision: 'approved' | 'rejected', comment: string) => void;
  busy: boolean; canApprove: boolean; onNavigation: () => void;
  session: ChatSession; onSession: (session: ChatSession) => void;
  onSent: (text: string) => void; onAddCaseDetails?: () => void; onVersionChange: () => void;
}) {
  const plainConversation = detail.kind === 'conversation';
  const [context, setContext] = useState<ContextSection | null>(null);
  const [attachments, setAttachments] = useState<Array<{ document: CaseDocument; preview?: string }>>([]);
  const [uploading, setUploading] = useState(false);
  const [attachmentError, setAttachmentError] = useState('');
  const previews = useRef<string[]>([]);
  const currentCase = useRef(detail.id);
  currentCase.current = detail.id;
  useEffect(() => () => { previews.current.forEach(url => URL.revokeObjectURL(url)); }, []);
  useEffect(() => { previews.current.forEach(url => URL.revokeObjectURL(url)); previews.current = []; setAttachments([]); setUploading(false); setAttachmentError(''); }, [detail.id]);
  const uploadAttachment = async (file: File) => {
    let contentType: string;
    try { contentType = attachmentContentType(file); } catch (error) { setAttachmentError(String(error)); return; }
    if (!file.size || file.size > 5 * 1024 * 1024) { setAttachmentError('Choose a file up to 5 MB.'); return; }
    if (contentType.startsWith('image/') && settings.model !== 'deepseek-flash') { setAttachmentError('图片输入目前需要 DeepSeek Flash。'); return; }
    if (attachments.length >= 4 || attachments.reduce((total, item) => total + item.document.bytes, 0) + file.size > 5 * 1024 * 1024) { setAttachmentError('Choose up to four attachments, 5 MB total.'); return; }
    setUploading(true); setAttachmentError('');
    try {
      const data = await new Promise<string>((resolve, reject) => { const reader = new FileReader(); reader.onload = () => resolve(String(reader.result).split(',')[1]); reader.onerror = reject; reader.readAsDataURL(file); });
      const saved = await api.uploadDocument(detail.id, { request_id: crypto.randomUUID(), filename: file.name, content_type: contentType, data_base64: data });
      if (currentCase.current !== detail.id) return;
      const preview = contentType.startsWith('image/') ? URL.createObjectURL(file) : undefined;
      if (preview) previews.current.push(preview);
      setAttachments(current => [...current, { document: saved, preview }]);
    } catch (reason) { if (currentCase.current === detail.id) setAttachmentError(reason instanceof Error ? reason.message : 'Upload failed.'); }
    finally { if (currentCase.current === detail.id) setUploading(false); }
  };
  const [atBottom, setAtBottom] = useState(true);
  const scroll = useRef<HTMLDivElement>(null);
  const follow = useRef(true);
  const composer = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => { if (composer.current) resizeComposer(composer.current); }, [session.draft]);
  useEffect(() => {
    const node = composer.current;
    if (!node) return;
    let width = node.clientWidth;
    const resize = () => resizeComposer(node);
    const observer = new ResizeObserver(() => {
      if (width !== node.clientWidth) { width = node.clientWidth; resize(); }
    });
    observer.observe(node);
    window.addEventListener('resize', resize);
    window.visualViewport?.addEventListener('resize', resize);
    return () => { observer.disconnect(); window.removeEventListener('resize', resize); window.visualViewport?.removeEventListener('resize', resize); };
  }, []);
  const conversation = useConversation(detail.id, !demoMode);
  const requestedVersion = useRef(detail.version);
  useEffect(() => {
    const version = conversation.data?.case_version;
    if (version && version > detail.version && version > requestedVersion.current) {
      requestedVersion.current = version;
      onVersionChange();
    }
  }, [conversation.data?.case_version, detail.version, onVersionChange]);
  const [draftSettings, setDraftSettings] = useState<AgentSettings | null>(null);
  const settings: AgentSettings = draftSettings ?? conversation.data?.settings ?? { approval: 'ask', model: 'deepseek-flash', effort: 'high' };
  const settingsRevision = useRef(0);
  const changeSettings = (next: AgentSettings) => {
    const revision = ++settingsRevision.current;
    setDraftSettings(next);
    if (!demoMode) void conversation.updateSettings(next).then(() => {
      if (settingsRevision.current === revision) setDraftSettings(null);
    });
  };
  const proposal = detail.proposals[0];
  const savedMessages = conversation.data?.items ?? [];
  const inlineProposal = savedMessages.some(message => message.response?.permissions?.some(permission => permission.tool === 'request_proposal_approval' && permission.arguments.proposal_id === proposal?.id));
  const beforeProposal = savedMessages.filter(message => !proposal || Date.parse(message.created_at) <= Date.parse(proposal.created_at));
  const afterProposal = savedMessages.filter(message => proposal && Date.parse(message.created_at) > Date.parse(proposal.created_at));
  const fixture = demoMode ? demoAgentProposal(detail) : undefined;
  const action = fixture?.actions[0];
  const running = demoMode && run?.status === 'running';
  const paused = demoMode && run?.status === 'paused';
  const complete = demoMode && run?.status === 'complete' && run.finalized;
  const finishing = demoMode && run?.status === 'complete' && !run.finalized;
  const started = demoMode && !!fixture && !!run && run.status !== 'idle';
  const resultReady = (stage: number) => complete || started && (run?.activeStage ?? 0) > stage;
  const status = !demoMode && conversation.sending ? 'Saving message' : running ? steps[run.activeStage] : paused ? 'Analysis paused' : finishing ? 'Preparing the proposal'
    : detail.status === 'approved' ? 'Decision recorded' : detail.status === 'rejected' ? 'Revision required'
      : proposal ? 'Awaiting your review' : 'Ready for analysis';
  const latestResponse = savedMessages.at(-1)?.response;
  const scrollSignal = `${run?.activeStage}:${run?.status}:${session.messages.length}:${savedMessages.at(-1)?.id}:${latestResponse?.text.length}:${latestResponse?.state}:${latestResponse?.tools.length}:${detail.approvals.length}:${proposal?.id}`;
  const previousScrollSignal = useRef(scrollSignal);

  useEffect(() => {
    if (previousScrollSignal.current === scrollSignal) return;
    previousScrollSignal.current = scrollSignal;
    if (follow.current && scroll.current) scroll.current.scrollTop = scroll.current.scrollHeight;
  }, [scrollSignal]);

  useEffect(() => {
    const content = scroll.current?.querySelector('.message-list');
    if (!content) return;
    const observer = new ResizeObserver(() => {
      const answer = content.querySelector('.answer-text[data-streaming=true]');
      if (follow.current && scroll.current && answer?.textContent?.trim()) scroll.current.scrollTop = scroll.current.scrollHeight;
    });
    observer.observe(content);
    return () => observer.disconnect();
  }, []);

  const send = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const text = session.draft.trim();
    if ((!text && !attachments.length) || uploading || running || finishing) return;
    if (!demoMode) {
      follow.current = true;
      void conversation.send(text, settings, attachments.map(item => item.document.id)).then(saved => { if (saved) { previews.current.forEach(url => URL.revokeObjectURL(url)); previews.current = []; setAttachments([]); onSession({ ...session, draft: '' }); onSent(text); composer.current?.focus(); } });
      return;
    }
    let response: string;
    if (!fixture) response = 'There is no analysis fixture for this case yet. The message is kept in this local demo conversation; no agent or ERP request was sent.';
    else if (!proposal && run?.status === 'idle') {
      response = 'The prepared case is ready for simulation. It compares the existing order with the delivery instruction. The analysis has not run yet, and your message does not change the fixture values.';
    } else if (paused) response = 'The simulation is paused. Resume it to finish the current analysis; the existing proposal has not changed.';
    else if (/evidence|source|document|why|依据|证据|为什么/i.test(text)) {
      const source = fixture.evidence[0];
      response = `The synthetic instruction (${source.revision}, ${source.locator}) requests a replacement total of ${number(action!.proposed_quantity)} units, due ${action!.proposed_delivery_date}. It applies to ${detail.customer_code} / ${detail.part_number} / Rev ${detail.revision}. This is the basis of the prepared proposal.`;
    } else if (/erp|order|baseline|订单|基线/i.test(text)) response = `The synthetic ERP baseline is ${action!.sales_order}, row ${action!.item_row}: ${number(action!.expected_quantity)} units, due ${action!.expected_delivery_date}. No ERP write has been sent.`;
    else if (/approv|status|审批|状态|执行/i.test(text)) response = `The case is ${detail.status.replaceAll('_', ' ')}. ${proposal ? `Proposal v${proposal.case_version} is bound to digest ${proposal.digest.slice(0, 8)}.` : 'No proposal is ready yet.'} A recorded approval does not execute an ERP change.`;
    else response = 'The prepared order comparison, evidence and approval state are available in this simulation. Your message leaves the proposal unchanged.';
    follow.current = true;
    onSession({ draft: '', messages: [...session.messages, { id: crypto.randomUUID(), role: 'user', text }, { id: crypto.randomUUID(), role: 'assistant', text: response }] });
    onSent(text);
    composer.current?.focus();
  };

  return <>
    <header className="chat-header">
      <button className="icon-button conversation-toggle" onClick={onNavigation} aria-label="Open conversations" title="Open conversations"><Menu size={19} /></button>
      <div className="chat-heading"><div className="chat-breadcrumb"><span>{plainConversation ? 'Conversations' : 'Cases'}</span>{!plainConversation && <><ChevronRight size={12} /><span>{detail.customer_code}</span><span className="case-version">v{detail.version}</span></>}</div><h1>{detail.title}</h1></div>
      <div className="chat-header-actions"><button className="icon-button" aria-label={plainConversation ? 'Open conversation details' : 'Open case details'} title={plainConversation ? 'Conversation details' : 'Case details'} onClick={() => setContext('order')}><PanelRight size={18} /></button></div>
    </header>
    <div className="chat-body">
      <div className="chat-scroll" ref={scroll} onScroll={event => { const node = event.currentTarget; follow.current = node.scrollHeight - node.scrollTop - node.clientHeight < 64; setAtBottom(follow.current); }}>
        <div className="message-list" aria-label="Case conversation">
          {!plainConversation && <div className="conversation-date">{demoMode ? 'Demo conversation' : 'Case record'}<span />{detail.part_number} / Rev {detail.revision}</div>}
          {fixture && action && <article className="user-message initial-request" aria-label="Customer request"><div className="user-message-meta">You<span>Change request</span></div><div className="user-bubble"><p>Review the {detail.customer_code} delivery change: replace the order total with <strong>{number(action.proposed_quantity)} units</strong>, due <strong>{date(action.proposed_delivery_date)}</strong>. Check the current order and prepare a proposal for approval.</p><button className="request-attachment" onClick={() => setContext('evidence')}><FileText size={16} /><span>Delivery instruction<small>{fixture.evidence[0].revision} / {detail.customer_code}</small></span><ChevronRight size={14} /></button></div></article>}
          {!demoMode && <>
            {conversation.data?.next_before && <button className="text-button earlier-messages" disabled={conversation.loading} onClick={() => { follow.current = false; void conversation.loadEarlier(); }}>Load earlier messages</button>}
            {conversation.loading && !conversation.data && <p className="conversation-feedback" role="status">Loading conversation...</p>}
            {beforeProposal.map(message => <SavedMessage key={message.id} caseId={detail.id} onDecision={conversation.refresh} message={message} own={!!conversation.data?.can_send} />)}
          </>}
          {(demoMode && !plainConversation || proposal && !inlineProposal) && <article className="assistant-message" aria-label={demoMode ? 'Agent response' : 'Saved proposal'}>
            {demoMode ? <AgentIdentity demoMode /> : <div className="agent-identity"><span className="agent-avatar"><FileText size={17} /></span><strong>Change proposal</strong></div>}
            <div className="assistant-content">
              {!demoMode ? <p className="agent-intro">Saved proposal</p>
                : !fixture ? <p className="agent-intro">This case has no prepared analysis. No ERP baseline or knowledge results are available.</p>
                  : <p className="agent-intro">{complete ? 'I checked the request against the order and its supporting documents.' : 'I will check the order baseline and the applicable delivery instruction before preparing the change.'}{(running || paused || finishing) && <span className="analysis-in-progress">{paused ? ' Analysis paused.' : ' Analysis in progress.'}</span>}</p>}
              {started && <>
                <details className="analysis-summary"><summary><span className="summary-icon">{running || finishing ? <LoaderCircle size={14} className="spinning" /> : paused ? <Pause size={14} /> : <Check size={14} />}</span><span>{complete ? 'Analysis complete' : status}</span><span className="step-counter">{complete ? steps.length : run!.activeStage + 1}/{steps.length} steps</span><ChevronDown size={13} /></summary><ol>{steps.map((step, index) => <li key={step} className={complete || run!.activeStage > index ? 'step-done' : run!.activeStage === index ? 'step-current' : ''}>{complete || run!.activeStage > index ? <Check size={13} /> : <span className="step-number">{index + 1}</span>}{step}</li>)}</ol></details>
                {(complete || run!.activeStage >= 1) && <div className="chat-tool-calls" aria-label="Agent tool calls">
                  <ToolCall name="Read order" source="ERPNext" tool="get_demand_baseline" target={action!.sales_order} done={resultReady(1)} paused={paused} icon={<Database size={15} />}>
                    <dl className="tool-result"><div><dt>Sales order / row</dt><dd>{action!.sales_order} / {action!.item_row}</dd></div><div><dt>Current quantity</dt><dd>{number(action!.expected_quantity)} units</dd></div><div><dt>Delivery</dt><dd>{action!.expected_delivery_date}</dd></div><div><dt>ERP modified</dt><dd>{action!.expected_erp_modified}</dd></div></dl><button className="text-button" onClick={() => setContext('order')}>Inspect order<ArrowRight size={13} /></button>
                  </ToolCall>
                  {resultReady(1) && <p className="agent-tool-update">The order currently has {number(action!.expected_quantity)} units, due {date(action!.expected_delivery_date)}. I will check the delivery instruction for {detail.part_number} / Rev {detail.revision} before drafting the change.</p>}
                  {(complete || run!.activeStage >= 3) && <ToolCall name="Search evidence" source="OpenViking" tool="search_case_knowledge" target={`${fixture!.evidence.length} references / ${detail.part_number}`} done={resultReady(3)} paused={paused} icon={<BookOpen size={15} />}>
                    <p className="tool-scope">{detail.customer_code} / {detail.part_number} / Rev {detail.revision}</p><p className="tool-excerpt">Replace the order total with {number(action!.proposed_quantity)} units. Requested delivery: {action!.proposed_delivery_date}.</p><button className="text-button" onClick={() => setContext('evidence')}>Inspect evidence<ArrowRight size={13} /></button>
                  </ToolCall>}
                </div>}
              </>}
              {demoMode && fixture && !started && <button className="button primary-button start-analysis" onClick={onRun}><Play size={14} />Run simulation</button>}
              {proposal && (!demoMode || !running && !paused && !finishing) && <>
                <p className="agent-result">{proposal.summary} {detail.quantity_basis === 'replacement' && 'This replaces the existing total; it is not an additional quantity.'}</p>
                <ProposalReview detail={detail} proposal={proposal} onDecide={onDecide} canApprove={canApprove} busy={busy} onEvidence={() => setContext('evidence')} />
              </>}
              {fixture && <div className="response-actions"><button className="text-button" onClick={() => setContext('flow')}><GitBranch size={13} />Execution details</button>{complete && <button className="icon-button" title="Replay simulation" aria-label="Replay simulation" onClick={onRun}><RotateCcw size={14} /></button>}{paused && <button className="text-button" onClick={onRun}><Play size={13} />Resume analysis</button>}</div>}
            </div>
          </article>}
          {!demoMode && afterProposal.map(message => <SavedMessage key={message.id} caseId={detail.id} onDecision={conversation.refresh} message={message} own={!!conversation.data?.can_send} />)}
          {session.messages.map(message => message.role === 'user' ? <article className="user-message followup-message" key={message.id}><div className="user-message-meta">You</div><div className="user-bubble"><p>{message.text}</p></div></article> : <article className="assistant-message followup-message" key={message.id}><AgentIdentity demoMode={demoMode} /><div className="assistant-content"><p className="followup-text">{message.text}</p></div></article>)}
        </div>
      </div>
      {!atBottom && <button className="jump-to-latest icon-button" aria-label="Jump to latest message" title="Jump to latest message" onClick={() => { follow.current = true; scroll.current?.scrollTo({ top: scroll.current.scrollHeight, behavior: window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 'instant' : 'smooth' }); }}><ArrowDown size={17} /></button>}
    </div>
    <footer className="chat-footer">
      <div className="composer-wrap">
        {!demoMode && conversation.error && <p className="conversation-error" role="alert">{conversation.error}<button type="button" className="text-button" onClick={conversation.refresh}>Reload history</button></p>}
        {(demoMode && !plainConversation || proposal || conversation.sending) && <div className="composer-status" role="status"><span>{running || conversation.sending ? <LoaderCircle className="spinning" size={13} /> : paused ? <Pause size={13} /> : <ShieldCheck size={13} />}{status}</span>{demoMode && <span>No external calls</span>}</div>}
        <form className="chat-composer" onSubmit={send} aria-busy={conversation.savingSettings}>
          {!!attachments.length && <div className="draft-attachments" aria-label="Message attachments">{attachments.map(({ document, preview }) => <div className="draft-attachment" key={document.id}>{preview ? <img src={preview} alt="" /> : <FileText size={17} />}<span title={document.filename}>{document.filename}</span><button type="button" className="icon-button" aria-label={`Remove ${document.filename}`} title="移除附件" onClick={() => { setAttachments(items => items.filter(item => item.document.id !== document.id)); if (preview) { URL.revokeObjectURL(preview); previews.current = previews.current.filter(url => url !== preview); } }}><X size={14} /></button></div>)}</div>}
          {uploading && <p className="attachment-feedback" role="status"><LoaderCircle size={13} className="spinning" />上传中...</p>}
          {attachmentError && <p className="attachment-feedback attachment-error" role="alert">{attachmentError}</p>}
          <textarea ref={composer} aria-label="Message agent" placeholder={demoMode || conversation.data?.can_send ? '随心输入' : 'Read-only conversation'} value={session.draft} rows={2} maxLength={4000} disabled={!demoMode && (!conversation.data?.can_send || conversation.sending)} onChange={event => onSession({ ...session, draft: event.target.value })} onKeyDown={event => { if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing && event.keyCode !== 229) { event.preventDefault(); event.currentTarget.form?.requestSubmit(); } }} />
          <ComposerControls settings={settings} onSettings={changeSettings} config={conversation.data?.config} context={demoMode ? undefined : conversation.data?.context_usage} disabled={!demoMode && (!conversation.data?.can_send || conversation.sending)} canUpload={!demoMode && !!conversation.data?.can_send && !conversation.sending && !uploading && attachments.length < 4} active={!!running || conversation.active} canSend={!finishing && !uploading && (!!session.draft.trim() || !!attachments.length) && (demoMode || !!conversation.data?.can_send && !conversation.sending)} onDocuments={file => void uploadAttachment(file)} onStop={() => { if (demoMode) onPause(); else void conversation.cancel(); }} onTranscript={text => onSession({ ...session, draft: `${session.draft}${text}`.slice(0,4000) })} />
        </form>
        {demoMode && <div className="composer-note">Simulated conversation. Changes stay in this browser session.</div>}
      </div>
    </footer>
    {context && <ChatContextPanel detail={detail} events={events} run={run} demoMode={demoMode} section={context} onSection={setContext} onClose={() => setContext(null)} onRun={onRun} onPause={onPause} onAddCaseDetails={onAddCaseDetails && !conversation.active && !conversation.sending ? () => { setContext(null); onAddCaseDetails(); } : undefined} />}
  </>;
}

function AgentIdentity({ demoMode }: { demoMode: boolean }) {
  return <div className="agent-identity"><span className="agent-avatar"><Bot size={17} /></span><strong>Forge Agent</strong>{demoMode && <span>Demo</span>}</div>;
}

function SavedMessage({ message, own, caseId, onDecision }: { message: ConversationMessage; own: boolean; caseId: string; onDecision: () => void }) {
  const response = message.response;
  return <>
    {message.operation !== 'compact' && <article className="user-message followup-message"><div className="user-message-meta">{own ? 'You' : 'Case owner'}<span><Check size={12} />Saved</span></div><div className="user-bubble">{message.text && <p>{message.text}</p>}{!!message.attachments?.length && <div className="message-attachments">{message.attachments.map(item => <button key={item.id} type="button" title={`下载 ${item.filename}`} onClick={() => void api.downloadDocument(item)}><Paperclip size={14} /><span>{item.filename}</span></button>)}</div>}</div></article>}
    {response && <article className="assistant-message followup-message" aria-label="Agent response"><AgentIdentity demoMode={false} /><div className="assistant-content">
      <AgentReply response={response} compacting={message.operation === 'compact'} caseId={caseId} canDecide={own} onDecision={onDecision} />
    </div></article>}
  </>;
}

function ToolCall({ name, source, tool, target, done, paused, icon, children }: { name: string; source: string; tool: string; target: string; done: boolean; paused: boolean; icon: ReactNode; children: ReactNode }) {
  return <details className={`chat-tool ${done ? 'complete' : 'pending'}`}><summary>{icon}<strong>{name}</strong><span className="tool-target">{target}</span><span className="tool-state">{done ? <Check size={13} /> : paused ? <Pause size={13} /> : <LoaderCircle size={13} className="spinning" />}<span>{done ? 'Complete' : paused ? 'Paused' : 'Running'}</span></span><ChevronRight className="tool-chevron" size={13} /></summary><div className="chat-tool-output"><div className="tool-function"><code>{tool}</code><span>{source} / Simulated</span></div>{done ? children : <p className="tool-excerpt">{paused ? 'Paused before a result was received.' : 'Waiting for the simulated result...'}</p>}</div></details>;
}

function ProposalReview({ detail, proposal, onDecide, canApprove, busy, onEvidence }: {
  detail: CaseDetail; proposal: Proposal; onDecide: (decision: 'approved' | 'rejected', comment: string) => void;
  canApprove: boolean; busy: boolean; onEvidence: () => void;
}) {
  const [comment, setComment] = useState('');
  const [showComment, setShowComment] = useState(false);
  const approval = detail.approvals.find(item => item.proposal_id === proposal.id);
  const pending = detail.status === 'awaiting_approval';
  return <section className="inline-proposal" aria-label="Change proposal">
    <div className="proposal-heading"><span className="proposal-icon"><FileText size={18} /></span><div><h2>Order change proposal</h2><span>Proposal v{proposal.case_version}<span className="proposal-separator">/</span><code>{proposal.digest.slice(0, 8)}</code></span></div><span className="proposal-state">{pending ? 'Review required' : detail.status === 'approved' ? 'Approved' : detail.status === 'rejected' ? 'Rejected' : 'Saved'}</span></div>
    {proposal.actions.map((action, index) => <div className="proposal-action" key={`${action.sales_order}-${action.item_row}-${index}`}><div className="proposal-order"><Database size={12} />{action.sales_order}<span>Row {action.item_row}</span></div><div className="proposal-diff"><div className="diff-heading"><span>Field</span><span>Current</span><span /><span>Proposed</span></div><div><span>Quantity</span><span>{number(action.expected_quantity)} <small>units</small></span><ArrowRight size={13} /><strong>{number(action.proposed_quantity)} <small>units</small></strong></div><div><span>Delivery</span><span>{date(action.expected_delivery_date)}</span><ArrowRight size={13} /><strong>{date(action.proposed_delivery_date)}</strong></div></div></div>)}
    <button className="proposal-evidence" onClick={onEvidence}><BookOpen size={14} /><span>{proposal.evidence.length} evidence references</span><ChevronRight size={14} /></button>
    {approval ? <div className="approval-receipt" role="status">{approval.decision === 'approved' ? <Check size={18} /> : <X size={18} />}<div><strong>{approval.decision === 'approved' ? 'Approval recorded' : 'Proposal rejected'}</strong><p>{approval.comment || (approval.decision === 'approved' ? 'ERP execution remains pending.' : 'A revised proposal is required.')}</p><small>{approval.actor_id} / No ERP write sent</small></div></div> : pending && canApprove ? <form className="inline-decision" onSubmit={event => { event.preventDefault(); onDecide('approved', comment); }}><p><ShieldCheck size={13} />Approval records this exact proposal. ERP execution is pending.</p>{showComment && <textarea aria-label="Decision comment" value={comment} onChange={event => setComment(event.target.value)} maxLength={2000} placeholder="Add your decision comment" disabled={busy} autoFocus />}<div className="inline-decision-actions"><button type="button" className="text-button comment-toggle" onClick={() => setShowComment(!showComment)} aria-expanded={showComment}><MessageSquare size={14} /><span>{showComment ? 'Hide comment' : 'Add comment'}</span></button><button type="button" className="button secondary-button" disabled={busy} onClick={() => onDecide('rejected', comment)}><X size={14} />Reject</button><button type="submit" className="button primary-button" disabled={busy}><Check size={14} />{busy ? 'Recording...' : 'Approve proposal'}</button></div></form> : <div className="approval-unavailable"><ClipboardCheck size={15} />{pending ? 'Awaiting an authorized approver.' : detail.status === 'rejected' ? 'A revised proposal is required.' : 'No pending decision.'}</div>}
  </section>;
}
