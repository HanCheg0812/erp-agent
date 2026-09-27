import { useEffect, useRef, useState, type FormEvent } from 'react';
import { Activity, ArrowLeft, Check, CircleAlert, ExternalLink, HardDrive, Layers3, Play, Plus, RefreshCw, ShieldCheck } from 'lucide-react';
import { runtimeApi, type RuntimeCandidate, type RuntimeOverview } from './api';

type Tab = 'templates' | 'instances' | 'nodes' | 'releases';
const message = (error: unknown) => error instanceof Error ? error.message : 'Request failed';
const time = (value?: string | null) => value && !Number.isNaN(new Date(value).getTime()) && !value.startsWith('0001-') ? new Date(value).toLocaleString() : '—';
const short = (value?: string | null) => value ? value.length > 24 ? `${value.slice(0, 12)}…${value.slice(-8)}` : value : '—';

export function RuntimeManagement({ onBack, onOpenCase, onNavigation }: { onBack: () => void; onOpenCase: (id: string) => void; onNavigation: () => void }) {
  const [data, setData] = useState<RuntimeOverview | null>(null);
  const [selected, setSelected] = useState('');
  const [candidate, setCandidate] = useState<RuntimeCandidate | null>(null);
  const [tab, setTab] = useState<Tab>('templates');
  const [creating, setCreating] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [loading, setLoading] = useState(true);
  const createRequest = useRef<string | null>(null);

  const load = async () => {
    try {
      const overview = await runtimeApi.overview();
      setData(overview);
      setSelected(current => current || overview.candidates[0]?.id || '');
      setError('');
    } catch (reason) { setError(message(reason)); }
    finally { setLoading(false); }
  };
  useEffect(() => { void load(); }, []);
  useEffect(() => {
    if (candidate?.state !== 'building') return;
    const interval = window.setInterval(() => { void load(); }, 4000);
    return () => window.clearInterval(interval);
  }, [candidate?.state]);
  useEffect(() => {
    if (!selected) { setCandidate(null); return; }
    let active = true;
    runtimeApi.candidate(selected).then(value => { if (active) setCandidate(value); }).catch(reason => { if (active) setError(message(reason)); });
    return () => { active = false; };
  }, [selected, data]);

  const action = async (work: () => Promise<unknown>, label: string) => {
    setBusy(true); setError(''); setNotice('');
    try { await work(); setNotice(label); await load(); }
    catch (reason) { setError(message(reason)); }
    finally { setBusy(false); }
  };
  const create = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const values = new FormData(event.currentTarget);
    createRequest.current ??= crypto.randomUUID();
    const request_id = createRequest.current;
    void action(async () => {
      const value = await runtimeApi.create({ request_id, name: String(values.get('name')).trim(), image: String(values.get('image')).trim(), cpu_milli: Number(values.get('cpu')), memory_mib: Number(values.get('memory')) });
      createRequest.current = null; setSelected(value.id); setCreating(false);
    }, 'Candidate saved');
  };
  const adopt = (templateId: string) => void action(async () => {
    const value = await runtimeApi.adopt(templateId, { request_id: crypto.randomUUID(), name: `Imported ${templateId}` });
    setSelected(value.id);
  }, 'Template added as candidate');
  const openCanary = (id: string) => void action(async () => {
    const result = await runtimeApi.canary(id);
    onOpenCase(result.case_id);
  }, 'Test conversation ready');

  const active = data?.releases.find(value => value.id === data.active_release_id);
  const candidateActive = !!candidate && active?.candidate_id === candidate.id;
  const adopted = new Set(data?.candidates.map(value => value.template_id).filter(Boolean));
  return <div className="runtime-page">
    <header className="runtime-header">
      <div className="runtime-heading"><button className="icon-button runtime-mobile-menu" title="Open navigation" aria-label="Open navigation" onClick={onNavigation}><Layers3 size={18} /></button><button className="icon-button" title="Back to conversations" aria-label="Back to conversations" onClick={onBack}><ArrowLeft size={18} /></button><div><span className="runtime-kicker">Operations / Sandbox runtime</span><h1>Runtime management</h1></div></div>
      <button className="button secondary-button" onClick={() => void load()} disabled={busy || loading} title="Refresh runtime status"><RefreshCw size={15} />Refresh</button>
    </header>
    {error && <div className="runtime-alert" role="alert"><CircleAlert size={16} /><span>{error}</span><button className="text-button" onClick={() => void load()}>Retry</button></div>}
    {notice && <div className="runtime-notice" role="status"><Check size={15} />{notice}</div>}
    {loading && !data ? <div className="empty-state" role="status">Loading runtime status…</div> : data && <>
      <div className="runtime-summary">
        <div><span>Active template</span><strong title={active?.template_id ?? data.legacy_template_id}>{short(active?.template_id ?? data.legacy_template_id)}</strong><small>{active ? `Release ${short(active.id)}` : 'Legacy configuration'}</small></div>
        <div><span>Ready nodes</span><strong>{data.provider_available ? `${data.nodes.filter(node => node.healthy).length} / ${data.nodes.length}` : 'Unavailable'}</strong><small>Sampled {time(data.sampled_at)}</small></div>
        <div><span>Current instances</span><strong>{data.instances.length}</strong><small>{data.instances.filter(instance => instance.guest_ready).length} guest ready</small></div>
        <div><span>Config version</span><strong>{data.version}</strong><small>{data.releases.length} recorded releases</small></div>
      </div>
      <div className="runtime-tabs" role="tablist" aria-label="Runtime views">
        {([['templates', 'Templates', Layers3], ['instances', 'Instances', HardDrive], ['nodes', 'Nodes', Activity], ['releases', 'Releases', ShieldCheck]] as const).map(([key, label, Icon]) => <button role="tab" aria-selected={tab === key} className={tab === key ? 'active' : ''} key={key} onClick={() => setTab(key)}><Icon size={15} />{label}</button>)}
      </div>
      {tab === 'templates' && <div className="runtime-body">
        <div className="runtime-section-title"><div><h2>Forge candidates</h2><span>Build → test → activate</span></div><button className="button secondary-button" onClick={() => setCreating(value => !value)}><Plus size={15} />New candidate</button></div>
        {creating && <form className="runtime-form" onSubmit={create}><label>Name<input name="name" required minLength={3} maxLength={80} placeholder="Resident runtime v2" /></label><label>OCI image with SHA-256 digest<input name="image" required placeholder="127.0.0.1:5000/forge-agent-runtime:v2@sha256:…" /></label><label>CPU (m)<input name="cpu" type="number" min="250" max="2000" defaultValue="750" required /></label><label>Memory (MiB)<input name="memory" type="number" min="512" max="4096" defaultValue="1024" required /></label><button className="button primary-button" disabled={busy}>Save candidate</button></form>}
        <div className="runtime-candidates">
          <div className="runtime-candidate-list">{data.candidates.map(value => <button key={value.id} className={`runtime-candidate ${selected === value.id ? 'active' : ''}`} onClick={() => setSelected(value.id)}><span><strong>{value.name}</strong><small>{value.state}</small></span><code>{short(value.template_id)}</code></button>)}{!data.candidates.length && <p className="runtime-empty">No candidates</p>}</div>
          <div className="runtime-inspector">{candidate && candidate.id === selected ? <>
            <div className="runtime-inspector-head"><div><h3>{candidate.name}</h3><span>{candidate.source === 'adopt' ? 'Existing Cube template' : 'Forge build'}</span></div><span className={`runtime-state ${candidate.state}`}>{candidate.state}</span></div>
            <dl className="runtime-facts"><div><dt>Template ID</dt><dd>{candidate.template_id ?? 'Pending build'}</dd></div><div><dt>Image</dt><dd>{candidate.image}</dd></div><div><dt>Resources</dt><dd>{candidate.cpu_milli}m / {candidate.memory_mib} MiB</dd></div><div><dt>Build job</dt><dd>{candidate.job_id ?? '—'}</dd></div></dl>
            {candidate.build_request && <details className="runtime-request"><summary>Cube build request</summary><pre>{JSON.stringify(candidate.build_request, null, 2)}</pre></details>}
            <div className="runtime-gate"><span className={candidate.canary?.ready ? 'passed' : ''}>{candidate.canary?.ready ? <Check size={14} /> : <CircleAlert size={14} />}{candidate.canary?.ready ? 'Test passed' : 'Test pending'}</span><small>{candidate.canary?.reason}</small></div>
            <div className="runtime-actions">{candidate.state === 'draft' && <button className="button primary-button" disabled={busy} onClick={() => void action(() => runtimeApi.build(candidate.id), 'Build requested')}><Play size={15} />Build template</button>}{candidate.state === 'ready' && <button className="button secondary-button" disabled={busy} onClick={() => openCanary(candidate.id)}><ExternalLink size={15} />{candidate.canary?.case_id ? 'Open test conversation' : 'Create test conversation'}</button>}{candidate.state === 'ready' && <button className="button primary-button" disabled={busy || !candidate.canary?.ready || candidateActive} onClick={() => void action(() => runtimeApi.activate(candidate.id, data.version), 'Release activated')}><Check size={15} />{candidateActive ? 'Active' : 'Activate'}</button>}</div>
            {candidate.state === 'unconfirmed' && <p className="runtime-warning">Cube acknowledgement is unknown. Reconcile the build in Cube before creating another candidate.</p>}
            {candidate.error_code && <p className="runtime-warning">{candidate.error_code}</p>}
          </> : <p className="runtime-empty">Select a candidate</p>}</div>
        </div>
        <div className="runtime-section-title"><div><h2>Cube templates</h2><span>{data.templates.length} returned · resident contract required for Agent use</span></div></div>
        {!data.provider_available && <p className="runtime-warning">Cube inventory is unavailable. Database release records remain visible.</p>}
        <div className="runtime-table-wrap"><table className="runtime-table"><thead><tr><th>Template</th><th>Status</th><th>Image</th><th>Created</th><th></th></tr></thead><tbody>{data.templates.map(value => <tr key={value.template_id}><td><strong>{value.aliases?.join(', ') || value.template_id}</strong><small>{value.template_id}</small></td><td><span className={`runtime-state ${value.status.toLowerCase()}`}>{value.status}</span></td><td title={value.image}>{short(value.image)}</td><td>{time(value.created_at)}</td><td>{value.status === 'READY' && !adopted.has(value.template_id) && <button className="button secondary-button" disabled={busy} onClick={() => adopt(value.template_id)} title="Check this Cube template against the Forge resident JCode contract">Validate for JCode</button>}</td></tr>)}</tbody></table></div>
      </div>}
      {tab === 'instances' && <div className="runtime-body"><div className="runtime-section-title"><div><h2>Forge instances</h2><span>Active operation records</span></div></div><div className="runtime-table-wrap"><table className="runtime-table"><thead><tr><th>Instance</th><th>Template</th><th>State</th><th>Guest</th><th>Created</th></tr></thead><tbody>{data.instances.map(value => <tr key={value.id}><td title={value.sandbox_id ?? value.id}>{short(value.sandbox_id ?? value.id)}</td><td title={value.template_id ?? ''}>{short(value.template_id)}</td><td>{value.state}</td><td>{value.guest_ready ? 'Ready' : value.error_code ?? 'Pending'}</td><td>{time(value.created_at)}</td></tr>)}</tbody></table>{!data.instances.length && <p className="runtime-empty">No active instances</p>}</div></div>}
      {tab === 'nodes' && <div className="runtime-body"><div className="runtime-section-title"><div><h2>Cube nodes</h2><span>Current provider sample · no historical retention</span></div></div><div className="runtime-table-wrap"><table className="runtime-table"><thead><tr><th>Node</th><th>Health</th><th>CPU capacity</th><th>Memory capacity</th><th>Disk used</th><th>Sampled</th></tr></thead><tbody>{data.nodes.map(value => <tr key={value.ip}><td>{value.ip}</td><td><span className={`runtime-state ${value.healthy ? 'ready' : 'failed'}`} title={`Last reported state: ${value.status || 'Unknown'}`}>{value.healthy ? 'Healthy' : 'Unhealthy'}</span></td><td>{value.cpu_total ?? '—'}</td><td>{value.memory_mib ?? '—'} MiB</td><td>{typeof value.disk_percent === 'number' ? `${value.disk_percent.toFixed(1)}%` : '—'}</td><td>{time(value.sampled_at)}</td></tr>)}</tbody></table>{!data.nodes.length && <p className="runtime-empty">No node sample available</p>}</div></div>}
      {tab === 'releases' && <div className="runtime-body"><div className="runtime-section-title"><div><h2>Release history</h2><span>Append-only records</span></div></div><div className="runtime-table-wrap"><table className="runtime-table"><thead><tr><th>Released</th><th>Template</th><th>Candidate</th><th>Status</th><th></th></tr></thead><tbody>{data.releases.map(value => <tr key={value.id}><td>{time(value.created_at)}</td><td>{value.template_id}</td><td>{short(value.candidate_id)}</td><td>{value.id === data.active_release_id ? <span className="runtime-state ready">Active</span> : 'Previous'}</td><td>{value.id !== data.active_release_id && <button className="button secondary-button" disabled={busy} onClick={() => void action(() => runtimeApi.activate(value.candidate_id, data.version), 'Previous candidate reactivated')}>Reactivate</button>}</td></tr>)}</tbody></table>{!data.releases.length && <p className="runtime-empty">No releases</p>}</div></div>}
    </>}
  </div>;
}
