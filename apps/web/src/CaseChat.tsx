import { useEffect, useLayoutEffect, useRef, useState, type FormEvent } from 'react';
import {
  ArrowDown, ArrowRight, BookOpen, Bot, Check, ChevronRight,
  ClipboardCheck, Database, FileText, LoaderCircle, Menu, MessageSquare,
  Paperclip, PanelRight, ShieldCheck, X,
} from 'lucide-react';
import { api, type AuditEvent, type CaseDetail, type CaseDocument, type ConversationMessage, type Proposal } from './api';
import { ChatContextPanel, type ContextSection } from './ChatContextPanel';
import { useConversation } from './useConversation';
import { AgentReply } from './AgentReply';
import { ComposerControls } from './ComposerControls';
import type { AgentSettings } from '../../../packages/contracts/src/index';
import { attachmentContentType } from './attachmentTypes';

export type ChatSession = { draft: string };
const number = (value: number) => value.toLocaleString('en');
const date = (value: string) => new Intl.DateTimeFormat('en', { month: 'short', day: 'numeric', timeZone: 'UTC' }).format(new Date(`${value}T00:00:00Z`));

function resizeComposer(node: HTMLTextAreaElement) {
  node.style.maxHeight = `${Math.max(72, Math.min(280, (window.visualViewport?.height ?? window.innerHeight) * 0.35))}px`;
  node.style.height = '0px';
  node.style.height = `${node.scrollHeight}px`;
  node.style.overflowY = node.scrollHeight > node.clientHeight + 1 ? 'auto' : 'hidden';
}

export function CaseChat({ detail, events, onDecide, busy, canApprove, onNavigation, session, onSession, onSent, onAddCaseDetails, onVersionChange }: {
  detail: CaseDetail; events: AuditEvent[];
  onDecide: (decision: 'approved' | 'rejected', comment: string) => void;
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
  const conversation = useConversation(detail.id, true);
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
    void conversation.updateSettings(next).then(() => {
      if (settingsRevision.current === revision) setDraftSettings(null);
    });
  };
  const proposal = detail.proposals[0];
  const savedMessages = conversation.data?.items ?? [];
  const inlineProposal = savedMessages.some(message => message.response?.permissions?.some(permission => permission.tool === 'request_proposal_approval' && permission.arguments.proposal_id === proposal?.id));
  const beforeProposal = savedMessages.filter(message => !proposal || Date.parse(message.created_at) <= Date.parse(proposal.created_at));
  const afterProposal = savedMessages.filter(message => proposal && Date.parse(message.created_at) > Date.parse(proposal.created_at));
  const status = conversation.sending ? 'Saving message' : detail.status === 'approved' ? 'Decision recorded' : detail.status === 'rejected' ? 'Revision required'
      : proposal ? 'Awaiting your review' : 'Ready for analysis';
  const latestResponse = savedMessages.at(-1)?.response;
  const scrollSignal = `${savedMessages.at(-1)?.id}:${latestResponse?.text.length}:${latestResponse?.state}:${latestResponse?.tools.length}:${detail.approvals.length}:${proposal?.id}`;
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
    if ((!text && !attachments.length) || uploading) return;
    follow.current = true;
    void conversation.send(text, settings, attachments.map(item => item.document.id)).then(saved => { if (saved) { previews.current.forEach(url => URL.revokeObjectURL(url)); previews.current = []; setAttachments([]); onSession({ ...session, draft: '' }); onSent(text); composer.current?.focus(); } });
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
          {!plainConversation && <div className="conversation-date">Case record<span />{detail.part_number} / Rev {detail.revision}</div>}
          {conversation.data?.next_before && <button className="text-button earlier-messages" disabled={conversation.loading} onClick={() => { follow.current = false; void conversation.loadEarlier(); }}>Load earlier messages</button>}
          {conversation.loading && !conversation.data && <p className="conversation-feedback" role="status">Loading conversation...</p>}
          {beforeProposal.map(message => <SavedMessage key={message.id} caseId={detail.id} onDecision={conversation.refresh} message={message} own={!!conversation.data?.can_send} />)}
          {proposal && !inlineProposal && <article className="assistant-message" aria-label="Saved proposal">
            <div className="agent-identity"><span className="agent-avatar"><FileText size={17} /></span><strong>Change proposal</strong></div>
            <div className="assistant-content">
              <p className="agent-intro">Saved proposal</p>
              <p className="agent-result">{proposal.summary} {detail.quantity_basis === 'replacement' && 'This replaces the existing total; it is not an additional quantity.'}</p>
              <ProposalReview detail={detail} proposal={proposal} onDecide={onDecide} canApprove={canApprove} busy={busy} onEvidence={() => setContext('evidence')} />
            </div>
          </article>}
          {afterProposal.map(message => <SavedMessage key={message.id} caseId={detail.id} onDecision={conversation.refresh} message={message} own={!!conversation.data?.can_send} />)}
        </div>
      </div>
      {!atBottom && <button className="jump-to-latest icon-button" aria-label="Jump to latest message" title="Jump to latest message" onClick={() => { follow.current = true; scroll.current?.scrollTo({ top: scroll.current.scrollHeight, behavior: window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 'instant' : 'smooth' }); }}><ArrowDown size={17} /></button>}
    </div>
    <footer className="chat-footer">
      <div className="composer-wrap">
        {conversation.error && <p className="conversation-error" role="alert">{conversation.error}<button type="button" className="text-button" onClick={conversation.refresh}>Reload history</button></p>}
        {(proposal || conversation.sending) && <div className="composer-status" role="status"><span>{conversation.sending ? <LoaderCircle className="spinning" size={13} /> : <ShieldCheck size={13} />}{status}</span></div>}
        <form className="chat-composer" onSubmit={send} aria-busy={conversation.savingSettings}>
          {!!attachments.length && <div className="draft-attachments" aria-label="Message attachments">{attachments.map(({ document, preview }) => <div className="draft-attachment" key={document.id}>{preview ? <img src={preview} alt="" /> : <FileText size={17} />}<span title={document.filename}>{document.filename}</span><button type="button" className="icon-button" aria-label={`Remove ${document.filename}`} title="移除附件" onClick={() => { setAttachments(items => items.filter(item => item.document.id !== document.id)); if (preview) { URL.revokeObjectURL(preview); previews.current = previews.current.filter(url => url !== preview); } }}><X size={14} /></button></div>)}</div>}
          {uploading && <p className="attachment-feedback" role="status"><LoaderCircle size={13} className="spinning" />上传中...</p>}
          {attachmentError && <p className="attachment-feedback attachment-error" role="alert">{attachmentError}</p>}
          <textarea ref={composer} aria-label="Message agent" placeholder={conversation.data?.can_send ? '随心输入' : 'Read-only conversation'} value={session.draft} rows={2} maxLength={4000} disabled={!conversation.data?.can_send || conversation.sending} onChange={event => onSession({ ...session, draft: event.target.value })} onKeyDown={event => { if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing && event.keyCode !== 229) { event.preventDefault(); event.currentTarget.form?.requestSubmit(); } }} />
          <ComposerControls settings={settings} onSettings={changeSettings} config={conversation.data?.config} context={conversation.data?.context_usage} disabled={!conversation.data?.can_send || conversation.sending} canUpload={!!conversation.data?.can_send && !conversation.sending && !uploading && attachments.length < 4} active={conversation.active} canSend={!uploading && (!!session.draft.trim() || !!attachments.length) && !!conversation.data?.can_send && !conversation.sending} onDocuments={file => void uploadAttachment(file)} onStop={() => void conversation.cancel()} onTranscript={text => onSession({ ...session, draft: `${session.draft}${text}`.slice(0,4000) })} />
        </form>
      </div>
    </footer>
    {context && <ChatContextPanel detail={detail} events={events} section={context} onSection={setContext} onClose={() => setContext(null)} onAddCaseDetails={onAddCaseDetails && !conversation.active && !conversation.sending ? () => { setContext(null); onAddCaseDetails(); } : undefined} />}
  </>;
}

function AgentIdentity() {
  return <div className="agent-identity"><span className="agent-avatar"><Bot size={17} /></span><strong>Forge Agent</strong></div>;
}

function SavedMessage({ message, own, caseId, onDecision }: { message: ConversationMessage; own: boolean; caseId: string; onDecision: () => void }) {
  const response = message.response;
  return <>
    {message.operation !== 'compact' && <article className="user-message followup-message"><div className="user-message-meta">{own ? 'You' : 'Case owner'}<span><Check size={12} />Saved</span></div><div className="user-bubble">{message.text && <p>{message.text}</p>}{!!message.attachments?.length && <div className="message-attachments">{message.attachments.map(item => <button key={item.id} type="button" title={`下载 ${item.filename}`} onClick={() => void api.downloadDocument(item)}><Paperclip size={14} /><span>{item.filename}</span></button>)}</div>}</div></article>}
    {response && <article className="assistant-message followup-message" aria-label="Agent response"><AgentIdentity /><div className="assistant-content">
      <AgentReply response={response} compacting={message.operation === 'compact'} caseId={caseId} canDecide={own} onDecision={onDecision} />
    </div></article>}
  </>;
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
