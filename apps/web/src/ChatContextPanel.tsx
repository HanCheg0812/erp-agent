import { useEffect, useRef, useState } from 'react';
import { Activity, BookOpen, Database, FileText, Plus, X } from 'lucide-react';
import type { AuditEvent, CaseDetail } from './api';

export type ContextSection = 'evidence' | 'order' | 'activity';
const sections = [
  { id: 'evidence', label: 'Evidence', icon: BookOpen },
  { id: 'order', label: 'Order', icon: Database },
  { id: 'activity', label: 'Activity', icon: Activity },
] as const;

export function ChatContextPanel({ detail, events, section, onSection, onClose, onAddCaseDetails }: {
  detail: CaseDetail; events: AuditEvent[];
  section: ContextSection; onSection: (section: ContextSection) => void; onClose: () => void;
  onAddCaseDetails?: () => void;
}) {
  const plainConversation = detail.kind === 'conversation';
  const dialog = useRef<HTMLDialogElement>(null);
  const [selectedSource, setSelectedSource] = useState(0);
  const proposal = detail.proposals[0];
  const source = proposal?.evidence[selectedSource];
  const businessEvents = events.filter(event => !event.event_type.startsWith('sandbox.'));
  useEffect(() => { const element = dialog.current; element?.showModal(); return () => element?.close(); }, []);

  return <dialog ref={dialog} className="context-dialog" aria-labelledby="context-title" onCancel={event => { event.preventDefault(); onClose(); }}>
    <header className="context-header"><div><h2 id="context-title">{plainConversation ? 'Conversation details' : 'Case details'}</h2><p>{plainConversation ? detail.title : `${detail.customer_code} / ${detail.part_number} / Rev ${detail.revision}`}</p></div><button className="icon-button" aria-label="Close details" title="Close details" onClick={onClose}><X size={18} /></button></header>
    <nav className="context-tabs" aria-label="Detail views">{sections.map(({ id, label, icon: Icon }) => <button key={id} className={section === id ? 'selected' : ''} aria-pressed={section === id} onClick={() => onSection(id)}><Icon size={14} />{plainConversation && id === 'order' ? 'Details' : label}</button>)}</nav>
    <div className="context-content">
      {section === 'evidence' && <div className="evidence-context"><div className="context-section-title"><h3>Source documents</h3><span>{proposal?.evidence.length ?? 0} references</span></div>
        {proposal?.evidence.map((item, index) => <button key={`${item.document_id}-${item.locator}`} className={`source-reference ${index === selectedSource ? 'selected' : ''}`} aria-pressed={index === selectedSource} onClick={() => setSelectedSource(index)}><FileText size={19} /><span><strong>{`Document ${index + 1}`}</strong><small>{item.revision} / {item.locator}</small></span></button>)}
        {source ? <><div className="document-excerpt"><div className="document-letterhead"><span>{detail.customer_code}</span><span>{source.revision}</span></div><h3>Evidence reference</h3><p className="document-meta">{detail.part_number} / Engineering revision {detail.revision}</p><p className="document-reference-only">Original document content is not connected. Only the saved reference is available.</p><div className="document-location"><FileText size={12} />{source.locator}</div></div><dl className="context-metadata"><div><dt>Source</dt><dd>Saved proposal reference</dd></div><div><dt>Document ID</dt><dd><code>{source.document_id}</code></dd></div><div><dt>Retrieval scope</dt><dd>{detail.customer_code} / {detail.part_number} / Rev {detail.revision}</dd></div></dl></> : <p className="context-empty">No evidence references are available.</p>}
      </div>}
      {section === 'order' && (plainConversation
        ? <div className="order-context"><h3>Case details</h3><p className="context-empty">No case linked.</p>{onAddCaseDetails && <button className="button secondary-button" onClick={onAddCaseDetails}><Plus size={15} />Add case details</button>}</div>
        : <div className="order-context"><div className="context-section-title"><h3>Case context</h3><span>v{detail.version}</span></div><dl className="context-metadata"><div><dt>Demand</dt><dd>{detail.demand_type?.replaceAll('_', ' ')}</dd></div><div><dt>Quantity basis</dt><dd>{detail.quantity_basis}</dd></div><div><dt>Owner</dt><dd>{detail.owner_id}</dd></div><div><dt>State</dt><dd>{detail.status.replaceAll('_', ' ')}</dd></div></dl><div className="context-section-title order-baseline-title"><h3>Order baseline</h3><span>From proposal</span></div>{proposal?.actions.map((item, index) => <section className="order-baseline" key={`${item.sales_order}-${index}`}><h4><Database size={16} />{item.sales_order}</h4><dl className="context-metadata"><div><dt>Item row</dt><dd>{item.item_row}</dd></div><div><dt>Current quantity</dt><dd>{item.expected_quantity.toLocaleString('en')} units</dd></div><div><dt>Current delivery</dt><dd>{item.expected_delivery_date}</dd></div><div><dt>Expected ERP version</dt><dd>{item.expected_erp_modified}</dd></div></dl></section>)}{!proposal && <p className="context-empty">No order baseline is available.</p>}<p className="context-footnote">ERP revalidation and execution have not been performed.</p></div>)}
      {section === 'activity' && <div className="activity-context"><div className="context-section-title"><h3>Business activity</h3><span>{businessEvents.length} events</span></div><ol className="chat-audit">{businessEvents.map(event => <li key={event.id}><span className="audit-marker" /><div><strong>{event.event_type.replaceAll('.', ' / ')}</strong><p>{event.actor_id}</p><time dateTime={event.created_at}>{new Intl.DateTimeFormat('en', { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' }).format(new Date(event.created_at))}</time></div></li>)}</ol>{!businessEvents.length && <p className="context-empty">No business activity recorded.</p>}</div>}
    </div>
  </dialog>;
}
