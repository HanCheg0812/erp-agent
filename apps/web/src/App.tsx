import { useEffect, useMemo, useRef, useState, type FormEvent, type ReactNode } from 'react';
import {
  Brain, Check, ClipboardCheck, Clock3, LogOut,
  MessageSquare, Plus, RefreshCw, Search, ShieldCheck, X,
} from 'lucide-react';
import {
  api, clearToken, getToken, subscribeAuthChange,
  type AuditEvent, type CaseDetail, type CaseInput, type CaseRecord, type CaseStatus,
} from './api';
import { CaseChat, type ChatSession } from './CaseChat';
import { PersonalMemory } from './PersonalMemory';
import { AuthScreen } from './AuthScreen';

const statusLabels: Record<CaseStatus, string> = {
  analyzing: 'Analyzing', awaiting_approval: 'Awaiting approval', approved: 'Approved', rejected: 'Rejected',
};
const formatCompactDate = (value: string) => new Intl.DateTimeFormat('en', { month: 'short', day: 'numeric' }).format(new Date(value));
const errorMessage = (error: unknown) => error instanceof Error ? error.message : 'The request failed. Please try again.';
type Mode = 'connecting' | 'live' | 'error';

function App() {
  const [token, setSessionToken] = useState(getToken);
  const [notice, setNotice] = useState('');
  useEffect(() => subscribeAuthChange(message => {
    setSessionToken(getToken()); setNotice(message);
  }), []);
  if (!token) return <AuthScreen notice={notice} />;
  return <Workspace key={token} />;
}

function Workspace() {
  const [mode, setMode] = useState<Mode>('connecting');
  const [cases, setCases] = useState<CaseRecord[]>([]);
  const [selectedId, setSelectedId] = useState('');
  const [liveDetail, setLiveDetail] = useState<CaseDetail | null>(null);
  const [liveEvents, setLiveEvents] = useState<AuditEvent[]>([]);
  const [filter, setFilter] = useState<'all' | CaseStatus>('all');
  const [query, setQuery] = useState('');
  const [chatSessions, setChatSessions] = useState<Record<string, ChatSession>>({});
  const [roles, setRoles] = useState<string[]>([]);
  const [email, setEmail] = useState('');
  const [memoryOpen, setMemoryOpen] = useState(false);
  const [toast, setToast] = useState('');
  const [error, setError] = useState('');
  const [detailError, setDetailError] = useState('');
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [mobileNav, setMobileNav] = useState(false);
  const [modal, setModal] = useState<'create' | null>(null);
  const [refresh, setRefresh] = useState(0);
  const retries = useRef(new Map<string, string>());
  const conversationRequest = useRef<string | null>(null);
  const pending = useRef(false);
  const detail = liveDetail?.id === selectedId ? liveDetail : null;
  const events = liveEvents;

  useEffect(() => {
    if (mode !== 'connecting') return;
    let active = true;
    setError('');
    Promise.all([api.listCases(), api.me()]).then(([list, user]) => {
      if (!active) return;
      setCases(list.items); setRoles(user.roles); setEmail(user.email); setSelectedId(list.items[0]?.id ?? ''); setMode('live');
    }).catch(reason => { if (active) { setError(errorMessage(reason)); setMode('error'); } });
    return () => { active = false; };
  }, [mode]);

  useEffect(() => {
    if (mode !== 'live' || !selectedId) return;
    let active = true;
    setLoading(true); setDetailError('');
    Promise.all([api.getCase(selectedId), api.getEvents(selectedId)]).then(([nextDetail, audit]) => {
      if (!active) return;
      setLiveDetail(nextDetail); setLiveEvents([...audit.items].reverse());
      setCases(items => items.map(item => item.id === nextDetail.id ? nextDetail : item));
    }).catch(reason => { if (active) setDetailError(errorMessage(reason)); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [selectedId, mode, refresh]);

  useEffect(() => {
    if (!toast) return;
    const timeout = window.setTimeout(() => setToast(''), 5000);
    return () => window.clearTimeout(timeout);
  }, [toast]);

  useEffect(() => {
    if (!mobileNav) return;
    const close = (event: KeyboardEvent) => { if (event.key === 'Escape') setMobileNav(false); };
    window.addEventListener('keydown', close);
    return () => window.removeEventListener('keydown', close);
  }, [mobileNav]);

  const filteredCases = useMemo(() => cases.filter(item => (filter === 'all' || item.kind !== 'conversation' && item.status === filter)
    && `${item.title} ${item.customer_code ?? ''} ${item.part_number ?? ''}`.toLowerCase().includes(query.toLowerCase())), [cases, filter, query]);
  const counts = cases.reduce((acc, item) => { if (item.kind !== 'conversation') acc[item.status] += 1; return acc; }, { analyzing: 0, awaiting_approval: 0, approved: 0, rejected: 0 } as Record<CaseStatus, number>);
  const selectFilter = (value: 'all' | CaseStatus) => { setFilter(value); setQuery(''); setMobileNav(false); };

  const decide = async (decision: 'approved' | 'rejected', comment: string) => {
    const proposal = detail?.proposals[0];
    if (!detail || !proposal || detail.status !== 'awaiting_approval' || pending.current) return;
    pending.current = true; setSaving(true);
    const input = { proposal_id: proposal.id, proposal_digest: proposal.digest, expected_case_version: detail.version, decision, comment };
    const intent = JSON.stringify({ caseId: detail.id, ...input });
    const retryKey = retries.current.get(intent) ?? crypto.randomUUID();
    retries.current.set(intent, retryKey);
    try {
      await api.decide(detail.id, input, retryKey);
      retries.current.delete(intent); setRefresh(value => value + 1); setToast(`Proposal ${decision}.`);
    } catch (reason) { setToast(errorMessage(reason)); }
    pending.current = false; setSaving(false);
  };

  const startConversation = async () => {
    if (pending.current) return;
    pending.current = true; setSaving(true);
    conversationRequest.current ??= crypto.randomUUID();
    try {
      const record = await api.startConversation(conversationRequest.current);
      conversationRequest.current = null;
      setCases(items => [record, ...items.filter(item => item.id !== record.id)]);
      setSelectedId(record.id); selectFilter('all');
    } catch (reason) { setToast(errorMessage(reason)); }
    finally { pending.current = false; setSaving(false); }
  };

  const updateConversationTitle = async (id: string) => {
    try {
      const next = await api.getCase(id);
      setLiveDetail(current => current?.id === id ? next : current);
      setCases(items => items.map(item => item.id === id ? next : item));
    } catch (reason) { setToast(errorMessage(reason)); }
  };

  const addCaseDetails = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (pending.current || !detail) return;
    const values = Object.fromEntries(new FormData(event.currentTarget));
    const input: CaseInput = { title: String(values.title).trim(), customer_code: String(values.customer_code).trim(), part_number: String(values.part_number).trim(), revision: String(values.revision).trim(), demand_type: values.demand_type as CaseInput['demand_type'], quantity_basis: values.quantity_basis as CaseRecord['quantity_basis'] };
    if ([input.title, input.customer_code, input.part_number, input.revision].some(value => !value)) { setError('Complete all required fields.'); return; }
    pending.current = true; setSaving(true); setError('');
    try {
      const record = await api.setCaseContext(detail.id, input, detail.version);
      setLiveDetail(current => current?.id === record.id ? { ...current, ...record } : current);
      void api.getEvents(record.id).then(audit => setLiveEvents([...audit.items].reverse())).catch(() => {});
      setCases(items => items.map(item => item.id === record.id ? record : item)); setModal(null); setToast('Case details saved.');
    } catch (reason) { setError(errorMessage(reason)); }
    finally { pending.current = false; setSaving(false); }
  };

  const disconnect = async () => {
    const current = getToken();
    let notice = '';
    try { if (current) await api.logout(); }
    catch { notice = 'Signed out on this device. Server sign-out could not be confirmed.'; }
    if (current === getToken()) clearToken(notice);
  };

  const ready = mode === 'live';
  return <div className="chat-app">
    {mobileNav && <button className="chat-nav-backdrop" aria-label="Close conversations" onClick={() => setMobileNav(false)} />}
    <aside className={`conversation-sidebar ${mobileNav ? 'is-open' : ''}`} aria-label="Case conversations">
      <div className="sidebar-brand"><div className="brand-mark">F</div><strong>Forge</strong><button className="icon-button mobile-close" aria-label="Close conversations" title="Close conversations" onClick={() => setMobileNav(false)}><X size={18} /></button></div>
      <button className="button new-conversation" onClick={() => void startConversation()} disabled={!ready || saving || !roles.includes('planner')}><Plus size={16} />New conversation</button>
      <div className="conversation-search"><Search size={15} /><input aria-label="Search cases" placeholder="Search conversations" value={query} onChange={event => setQuery(event.target.value)} /><button className="icon-button" aria-label="Refresh cases" title="Refresh cases" onClick={() => setMode('connecting')} disabled={!ready}><RefreshCw size={14} /></button></div>
      <nav className="conversation-list" aria-label="Conversations">
        {filteredCases.map(item => <CaseRow key={item.id} item={item} selected={selectedId === item.id} onClick={() => { setSelectedId(item.id); setMobileNav(false); }} />)}
        {ready && !filteredCases.length && <div className="empty-state"><Search size={20} /><p>No matching conversations.</p>{cases.length > 0 && <button className="text-button" onClick={() => selectFilter('all')}>Clear filters</button>}</div>}
      </nav>
      <div className="sidebar-bottom">
        <button className={`review-queue ${filter === 'awaiting_approval' ? 'selected' : ''}`} onClick={() => setFilter(filter === 'awaiting_approval' ? 'all' : 'awaiting_approval')}><ClipboardCheck size={16} /><span>Needs your review</span><span>{counts.awaiting_approval}</span></button>
        <div className="sidebar-account"><ShieldCheck size={18} /><div><strong>{ready ? 'Account' : 'Connection pending'}</strong><small title={email}>{email}</small></div><button className="icon-button" title="Personal memory" aria-label="Personal memory" onClick={() => setMemoryOpen(true)} disabled={!ready}><Brain size={16} /></button><button className="icon-button" onClick={disconnect} aria-label="Sign out" title="Sign out"><LogOut size={17} /></button></div>
      </div>
    </aside>
    <main className="chat-main">
      {(!ready || loading || detailError || !detail) && <button className="icon-button conversation-toggle empty-navigation" onClick={() => setMobileNav(true)} aria-label="Open conversations" title="Open conversations"><MessageSquare size={18} /></button>}
      {!ready ? <div className="empty-state" role="status"><h2>{mode === 'connecting' ? 'Connecting' : 'Service unavailable'}</h2><p>{error || 'Loading conversations and account permissions.'}</p>{mode === 'error' && <button className="button secondary-button" onClick={() => setMode('connecting')}><RefreshCw size={15} />Retry</button>}</div>
        : loading && !detail ? <div className="empty-state" role="status">Loading conversation...</div>
        : detailError ? <div className="empty-state" role="alert"><p>{detailError}</p><button className="button secondary-button" onClick={() => setRefresh(value => value + 1)}><RefreshCw size={15} />Retry</button></div>
        : detail ? <CaseChat key={`${mode}-${detail.id}`} detail={detail} events={events} onDecide={decide} busy={saving} canApprove={roles.includes('approver')} onNavigation={() => setMobileNav(true)} session={chatSessions[detail.id] ?? { draft: '' }} onSession={session => setChatSessions(items => ({ ...items, [detail.id]: session }))} onSent={() => void updateConversationTitle(detail.id)} onVersionChange={() => setRefresh(value => value + 1)} onAddCaseDetails={roles.includes('planner') ? () => { setError(''); setModal('create'); } : undefined} />
        : <div className="empty-state"><MessageSquare size={26} /><h2>{cases.length ? 'No conversation selected' : 'No conversations yet'}</h2>{cases.length ? <button className="button secondary-button" onClick={() => setMobileNav(true)}>Open conversations</button> : roles.includes('planner') && <button className="button primary-button" disabled={saving} onClick={() => void startConversation()}><Plus size={16} />Start a conversation</button>}</div>}
    </main>
    {modal && detail && <Dialog title="Add case details" busy={saving} onClose={() => { setModal(null); setError(''); }}><form onSubmit={addCaseDetails} className="case-form">
      <label>Case title<input name="title" required maxLength={200} defaultValue={detail.title === 'New conversation' ? '' : detail.title} autoFocus /></label><div className="form-grid"><label>Customer code<input name="customer_code" required maxLength={100} /></label><label>Part number<input name="part_number" required maxLength={100} /></label><label>Revision<input name="revision" required maxLength={100} /></label><label>Demand type<select name="demand_type" defaultValue="order"><option value="order">Customer order</option><option value="forecast">Forecast</option><option value="delivery_instruction">Delivery instruction</option></select></label></div><label>Quantity basis<select name="quantity_basis" defaultValue="unknown"><option value="unknown">Unconfirmed</option><option value="replacement">Replacement total</option><option value="incremental">Incremental</option><option value="cumulative">Cumulative</option></select></label>
      {error && <p role="alert" className="form-error">{error}</p>}<div className="dialog-actions"><button type="button" className="button secondary-button" disabled={saving} onClick={() => { setModal(null); setError(''); }}>Cancel</button><button type="submit" className="button primary-button" disabled={saving}>{saving ? 'Saving...' : 'Save case details'}</button></div>
    </form></Dialog>}
    {memoryOpen && <PersonalMemory onClose={() => setMemoryOpen(false)} />}
    {toast && <div className="toast" role="status">{toast}<button className="icon-button" aria-label="Dismiss notification" title="Dismiss" onClick={() => setToast('')}><X size={14} /></button></div>}
  </div>;
}

function CaseRow({ item, selected, onClick }: { item: CaseRecord; selected: boolean; onClick: () => void }) {
  const Icon = item.status === 'approved' ? Check : item.status === 'rejected' ? X : item.status === 'awaiting_approval' ? ClipboardCheck : Clock3;
  return <button className={`conversation-item ${selected ? 'selected' : ''}`} onClick={onClick} aria-pressed={selected}>
    <div className="conversation-item-heading"><MessageSquare size={15} /><strong>{item.customer_code}</strong><span>{formatCompactDate(item.updated_at)}</span></div>
    <div className="conversation-title">{item.title}</div>
    {item.kind !== 'conversation' && <div className="conversation-item-meta"><span>{item.part_number} / Rev {item.revision}</span><span><Icon size={11} />{statusLabels[item.status]}</span></div>}
  </button>;
}

function Dialog({ title, children, onClose, busy }: { title: string; children: ReactNode; onClose: () => void; busy: boolean }) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => { const dialog = ref.current; dialog?.showModal(); return () => dialog?.close(); }, []);
  return <dialog ref={ref} className="dialog" aria-labelledby="dialog-title" onCancel={event => { event.preventDefault(); if (!busy) onClose(); }}><div className="dialog-header"><h2 id="dialog-title">{title}</h2><button className="icon-button" title="Close dialog" aria-label="Close dialog" disabled={busy} onClick={onClose}><X size={18} /></button></div>{children}</dialog>;
}

export default App;
