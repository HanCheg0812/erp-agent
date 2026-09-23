import { useEffect, useRef, useState } from 'react';
import { Download, LoaderCircle, Paperclip, X } from 'lucide-react';
import { api, type CaseDocument } from './api';
import { attachmentContentType } from './attachmentTypes';

export function CaseDocuments({ caseId, initialFile, onClose }: { caseId: string; initialFile?: File; onClose: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const [items, setItems] = useState<CaseDocument[]>([]);
  const [configured, setConfigured] = useState(false);
  const [loading, setLoading] = useState(true);
  const [uploading, setUploading] = useState(false);
  const busy = loading || uploading;
  const [error, setError] = useState('');
  const retry = useRef<{ file: File; id: string } | null>(null);
  const initialStarted = useRef(false);
  useEffect(() => {
    const node = dialog.current; node?.showModal(); let live = true;
    api.documents(caseId).then(result => { if (live) { setItems(current => [...current, ...result.items.filter(item => !current.some(saved => saved.id === item.id))]); setConfigured(result.configured); } }).catch(reason => { if (live) setError(String(reason.message)); }).finally(() => { if (live) setLoading(false); });
    return () => { live = false; node?.close(); };
  }, [caseId]);
  const upload = async (file: File) => {
    let contentType: string;
    try { contentType = attachmentContentType(file); } catch (reason) { setError(String(reason)); return; }
    if (!file.size || file.size > 5 * 1024 * 1024) { setError('Choose a file up to 5 MB.'); return; }
    if (retry.current?.file !== file) retry.current = { file, id: crypto.randomUUID() };
    setUploading(true); setError('');
    try {
      const base64 = await new Promise<string>((resolve, reject) => { const reader = new FileReader(); reader.onload = () => resolve(String(reader.result).split(',')[1]); reader.onerror = reject; reader.readAsDataURL(file); });
      const saved = await api.uploadDocument(caseId, { request_id: retry.current.id, filename: file.name, content_type: contentType, data_base64: base64 });
      setItems(current => [saved, ...current.filter(item => item.id !== saved.id)]); retry.current = null;
    } catch (reason) { setError(reason instanceof Error ? reason.message : 'Upload failed.'); }
    finally { setUploading(false); if (input.current) input.current.value = ''; }
  };
  useEffect(() => {
    if (initialFile && !initialStarted.current) {
      initialStarted.current = true;
      void upload(initialFile);
    }
  }, [initialFile]);
  return <dialog ref={dialog} className="dialog" aria-labelledby="documents-title" onCancel={event => { event.preventDefault(); if (!busy) onClose(); }}>
    <div className="dialog-header"><h2 id="documents-title">Case documents</h2><button className="icon-button" title="Close documents" aria-label="Close documents" disabled={busy} onClick={onClose}><X size={18} /></button></div>
    <div className="memory-content"><ul className="memory-list document-list">{items.map(item => <li key={item.id}><Paperclip size={16} /><div><strong>{item.filename}</strong><small>{Math.ceil(item.bytes / 1024)} KB / {item.state === 'ready' ? 'Uploaded, not indexed' : item.state}</small></div><button className="icon-button" title="Download document" aria-label={`Download ${item.filename}`} disabled={item.state !== 'ready'} onClick={() => void api.downloadDocument(item).catch(reason => setError(reason.message))}><Download size={16} /></button></li>)}</ul>
      {!items.length && <p className="memory-empty">{busy ? 'Loading documents...' : 'No documents uploaded.'}</p>}
      {error && <p className="form-error" role="alert">{error}</p>}<div className="dialog-actions">{retry.current && !busy && <button className="text-button" onClick={() => void upload(retry.current!.file)}>Retry upload</button>}<input ref={input} type="file" hidden accept=".pdf,.docx,.xlsx,.md,.txt,.png,.jpg,.jpeg,.webp" onChange={event => { const file = event.target.files?.[0]; if (file) void upload(file); }} /><button className="button primary-button" disabled={busy || !configured} onClick={() => input.current?.click()}>{busy ? <LoaderCircle size={15} className="spinning" /> : <Paperclip size={15} />}Upload document</button></div>
    </div>
  </dialog>;
}
