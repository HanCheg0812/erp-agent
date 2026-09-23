import { useEffect, useRef, useState } from 'react';
import { ArrowUp, Check, ChevronDown, ChevronRight, Circle, FileUp, ImagePlus, Mic, Plus, RotateCcw, Shield, Square, Zap } from 'lucide-react';
import type { AgentSettings, Conversation } from '../../../packages/contracts/src/index';

const effortLabels = { none: '关闭', low: '低', high: '高', max: '最高' };
const efforts = ['none', 'low', 'high', 'max'] as const;
const approvalLabels = { ask: '请求批准', auto: '帮我批准', full: '完全访问' };
const approvalDescriptions = { ask: '需要确认的操作由你批准', auto: '需要确认的操作交由独立审核', full: '直接执行账户已授权的操作' };
const modelName = (model: string) => model === 'deepseek-flash' ? 'DeepSeek Flash' : 'DeepSeek Pro';
const compactTokens = (value: number) => {
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(value >= 10_000_000 ? 0 : 2).replace(/\.0+$/, '')}m`;
  if (value >= 1_000) return `${(value / 1_000).toFixed(value >= 100_000 ? 0 : 1).replace(/\.0$/, '')}k`;
  return value.toLocaleString('zh-CN');
};
type Recognition = { lang: string; interimResults: boolean; onresult: ((event: { results: ArrayLike<ArrayLike<{ transcript: string }>> }) => void) | null; onerror: (() => void) | null; onend: (() => void) | null; start: () => void; stop: () => void };

export function ComposerControls({ settings, onSettings, config, context, disabled, canUpload, active, canSend, onDocuments, onStop, onTranscript }: {
  settings: AgentSettings; onSettings: (settings: AgentSettings) => void; config?: Conversation['config']; context?: Conversation['context_usage'];
  disabled: boolean; canUpload: boolean; active: boolean; canSend: boolean;
  onDocuments: (file: File) => void; onStop: () => void; onTranscript: (text: string) => void;
}) {
  const [menu, setMenu] = useState<'add' | 'access' | 'context' | 'reasoning' | 'model' | null>(null);
  const [listening, setListening] = useState(false);
  const [voiceError, setVoiceError] = useState('');
  const root = useRef<HTMLDivElement>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const imageInput = useRef<HTMLInputElement>(null);
  const speech = useRef<Recognition | null>(null);
  const Speech = (window as unknown as { SpeechRecognition?: new () => Recognition; webkitSpeechRecognition?: new () => Recognition }).SpeechRecognition ?? (window as unknown as { webkitSpeechRecognition?: new () => Recognition }).webkitSpeechRecognition;
  useEffect(() => {
    const close = (event: PointerEvent) => { if (!root.current?.contains(event.target as Node)) setMenu(null); };
    const escape = (event: KeyboardEvent) => { if (event.key === 'Escape') setMenu(null); };
    document.addEventListener('pointerdown', close); document.addEventListener('keydown', escape);
    return () => { document.removeEventListener('pointerdown', close); document.removeEventListener('keydown', escape); speech.current?.stop(); };
  }, []);
  const toggle = (next: typeof menu) => setMenu(menu === next ? null : next);
  const contextWindow = context?.model === settings.model ? context.window_tokens : config?.context_windows[settings.model] ?? context?.window_tokens ?? 0;
  const usedTokens = context?.used_tokens ?? 0;
  const contextRatio = contextWindow > 0 ? Math.min(usedTokens / contextWindow, 1) : 0;
  const contextPercent = Math.round(contextRatio * 100);
  const contextLabel = context?.measured ? `${contextPercent}% 已用` : '待测量';
  const dictate = () => {
    if (listening) { speech.current?.stop(); return; }
    if (!Speech) return;
    const recognition = new Speech(); speech.current = recognition;
    recognition.lang = 'zh-CN'; recognition.interimResults = false;
    recognition.onresult = event => onTranscript(Array.from(event.results).map(result => result[0].transcript).join(''));
    recognition.onerror = () => { setListening(false); setVoiceError('语音输入不可用，请检查麦克风权限。'); };
    recognition.onend = () => setListening(false);
    try { recognition.start(); setVoiceError(''); setListening(true); } catch { setVoiceError('无法启动语音输入。'); }
  };
  return <div className="composer-controls composer-reference" ref={root}>
    <button type="button" className="icon-button composer-add" aria-label="Add attachment" title="上传文件或图片" aria-expanded={menu === 'add'} disabled={!canUpload || active} onClick={() => toggle('add')}><Plus size={21} /></button>
    <button type="button" className="composer-access" aria-label="Agent approval mode" aria-expanded={menu === 'access'} disabled={disabled || active} onClick={() => toggle('access')}><Shield size={17} /><span>{approvalLabels[settings.approval]}</span></button>
    <div className="composer-model-group">
      {context ? <div className={`context-window-control ${menu === 'context' ? 'open' : ''}`}>
        <button type="button" className="context-usage-button" aria-label={`上下文窗口${contextLabel}`} aria-expanded={menu === 'context'} onClick={() => toggle('context')}>
          <span className="context-usage-ring" style={{ '--context-progress': `${contextRatio * 100}%` } as React.CSSProperties} />
        </button>
        <div className="context-usage-popover" role="status">
          <span>背景信息窗口：</span>
          <strong>{context.compacting ? '正在压缩' : contextLabel}</strong>
          <p>{context.measured ? `已用约 ${compactTokens(usedTokens)} 标记，共 ${compactTokens(contextWindow)}` : `总窗口 ${compactTokens(contextWindow)} 标记`}</p>
        </div>
      </div> : <Circle className="context-placeholder" size={14} />}
      <button type="button" className="composer-model" aria-label="Model and reasoning" aria-expanded={menu === 'reasoning' || menu === 'model'} disabled={disabled || active} onClick={() => menu === 'model' ? setMenu(null) : toggle('reasoning')}><span>{modelName(settings.model)}</span><span className="effort-label">{effortLabels[settings.effort]}</span><ChevronDown size={13} /></button>
    </div>
    <button type="button" className={`icon-button composer-mic ${listening ? 'listening' : ''}`} aria-label={listening ? 'Stop dictation' : 'Voice input'} title={!Speech ? '浏览器不支持语音输入' : listening ? '停止听写' : '语音输入'} disabled={!Speech || disabled || active} onClick={dictate}><Mic size={19} /></button>
    {active ? <button type="button" className="send-button" aria-label="Stop response" title="停止" onClick={onStop}><Square size={13} fill="currentColor" /></button> : <button type="submit" className="send-button" aria-label="Send message" title="发送" disabled={!canSend}><ArrowUp size={21} /></button>}
    <input ref={fileInput} type="file" hidden accept=".pdf,.docx,.xlsx,.md,.txt" onChange={event => { const file = event.target.files?.[0]; if (file) onDocuments(file); event.target.value = ''; }} />
    <input ref={imageInput} type="file" hidden accept="image/png,image/jpeg,image/webp,.png,.jpg,.jpeg,.webp" onChange={event => { const file = event.target.files?.[0]; if (file) onDocuments(file); event.target.value = ''; }} />
    {menu === 'add' && <div className="composer-menu add-menu"><button type="button" onClick={() => { setMenu(null); fileInput.current?.click(); }}><FileUp size={16} />上传文件</button><button type="button" onClick={() => { setMenu(null); imageInput.current?.click(); }}><ImagePlus size={16} />上传图片</button></div>}
    {menu === 'access' && <div className="composer-menu access-menu" role="group" aria-label="Approval modes">{(['ask','auto','full'] as const).map(approval => <button key={approval} type="button" aria-pressed={settings.approval === approval} onClick={() => { onSettings({ ...settings, approval }); setMenu(null); }}><Shield size={15} /><span>{approvalLabels[approval]}<small>{approvalDescriptions[approval]}</small></span>{settings.approval === approval && <Check size={14} />}</button>)}</div>}
    {menu === 'reasoning' && <div className="composer-menu reasoning-menu" role="group" aria-label="Model settings">
      <div className="reasoning-toolbar"><button type="button" className="icon-button" aria-label="Disable reasoning" title="关闭思考" onClick={() => onSettings({ ...settings, effort: 'none' })}><Zap size={17} /></button><button type="button" className="reasoning-selection" aria-label="Choose model" onClick={() => setMenu('model')}><span>{effortLabels[settings.effort]}<ChevronRight size={13} /></span><small>{modelName(settings.model)}</small></button><button type="button" className="icon-button" aria-label="Reset model settings" title="恢复默认模型与思考程度" onClick={() => onSettings({ ...settings, model: 'deepseek-flash', effort: 'high' })}><RotateCcw size={17} /></button></div>
      <div className="reasoning-slider" style={{ '--reasoning-progress': `${efforts.indexOf(settings.effort) / 3 * 100}%` } as React.CSSProperties}><div className="reasoning-track"><span />{efforts.map(effort => <i key={effort} />)}</div><input type="range" min="0" max="3" step="1" value={efforts.indexOf(settings.effort)} aria-label="思考程度" aria-valuetext={effortLabels[settings.effort]} onChange={event => onSettings({ ...settings, effort: efforts[Number(event.target.value)] })} /></div>
      <details className="endpoint-details"><summary>Responses API</summary><code>{config?.endpoint ?? '未连接'}</code></details>
    </div>}
    {menu === 'model' && <div className="composer-menu model-menu" role="group" aria-label="Choose model"><span className="menu-label">选择模型</span><button type="button" className="model-default" onClick={() => { onSettings({ ...settings, model: 'deepseek-flash' }); setMenu('reasoning'); }}><span>默认<small>DeepSeek Flash</small></span></button>{(config?.models ?? [settings.model]).map(model => <button key={model} type="button" aria-pressed={settings.model === model} onClick={() => { onSettings({ ...settings, model }); setMenu('reasoning'); }}><span>{modelName(model)}</span>{settings.model === model && <Check size={16} />}</button>)}</div>}
    {voiceError && <span className="voice-error" role="alert">{voiceError}</span>}
  </div>;
}
