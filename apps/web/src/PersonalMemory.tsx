import { useEffect, useRef, useState, type FormEvent } from 'react';
import { Brain, Plus, Trash2, X } from 'lucide-react';
import { api, type PersonalMemory as Memory } from './api';

export function PersonalMemory({ onClose }: { onClose: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [items, setItems] = useState<Memory[]>([]);
  const [text, setText] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(true);
  const retry = useRef<{ text: string; id: string } | null>(null);
  useEffect(() => {
    const node = dialog.current; node?.showModal(); let live = true;
    api.memories().then(result => { if (live) setItems(result.items); }).catch(reason => { if (live) setError(String(reason.message)); }).finally(() => { if (live) setBusy(false); });
    return () => { live = false; node?.close(); };
  }, []);
  const save = async (event: FormEvent) => {
    event.preventDefault(); if (!text.trim() || busy) return;
    const value = text.trim(); if (retry.current?.text !== value) retry.current = { text: value, id: crypto.randomUUID() };
    setBusy(true); setError('');
    try { const result = await api.saveMemory(value, retry.current.id); setItems(current => [result, ...current.filter(item => item.id !== result.id)]); setText(''); retry.current = null; }
    catch (reason) { setError(reason instanceof Error ? reason.message : 'Could not save memory.'); }
    finally { setBusy(false); }
  };
  const remove = async (id: string) => {
    setBusy(true); setError('');
    try { await api.deleteMemory(id); setItems(current => current.filter(item => item.id !== id)); }
    catch (reason) { setError(reason instanceof Error ? reason.message : 'Could not delete memory.'); }
    finally { setBusy(false); }
  };
  return <dialog ref={dialog} className="dialog memory-dialog" aria-labelledby="memory-title" onCancel={event => { event.preventDefault(); if (!busy) onClose(); }}>
    <div className="dialog-header"><h2 id="memory-title"><Brain size={17} />Personal memory</h2><button className="icon-button" title="Close memory" aria-label="Close memory" disabled={busy} onClick={onClose}><X size={18} /></button></div>
    <div className="memory-content"><ul className="memory-list">{items.map(item => <li key={item.id}><p>{item.text}</p><button className="icon-button" title="Delete memory" aria-label="Delete memory" disabled={busy} onClick={() => void remove(item.id)}><Trash2 size={15} /></button></li>)}</ul>
      {!items.length && <p className="memory-empty">{busy ? 'Loading memories...' : 'No saved memories.'}</p>}
      <form onSubmit={save} className="case-form"><label>Save a preference or working context<textarea value={text} onChange={event => setText(event.target.value)} maxLength={2000} rows={3} required disabled={busy} /></label>
        {error && <p className="form-error" role="alert">{error}</p>}<div className="dialog-actions"><span>{items.length} / 50</span><button className="button primary-button" type="submit" disabled={busy || !text.trim() || items.length >= 50}><Plus size={14} />Save memory</button></div>
      </form></div>
  </dialog>;
}
