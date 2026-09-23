import { useCallback, useEffect, useRef, useState } from 'react';
import { api, type Conversation, type ConversationMessage } from './api';
import type { AgentSettings } from '../../../packages/contracts/src/index';

const errorText = (reason: unknown) => reason instanceof Error ? reason.message : 'Conversation request failed';
const merge = (items: ConversationMessage[], additions: ConversationMessage[]) =>
  [...new Map([...items, ...additions].map(item => [item.id, item])).values()]
    .sort((a, b) => BigInt(a.id) < BigInt(b.id) ? -1 : BigInt(a.id) > BigInt(b.id) ? 1 : 0);

export function useConversation(caseId: string, enabled: boolean) {
  const [data, setData] = useState<Conversation | null>(null);
  const [loading, setLoading] = useState(enabled);
  const [sending, setSending] = useState(false);
  const [savingSettings, setSavingSettings] = useState(false);
  const [error, setError] = useState('');
  const [reload, setReload] = useState(0);
  const mounted = useRef(false);
  const pending = useRef(false);
  const settingsQueue = useRef(Promise.resolve(true));
  const settingsWrites = useRef(0);
  const settingsRevision = useRef(0);
  const retries = useRef(new Map<string, string>());
  const refresh = useCallback(() => setReload(value => value + 1), []);
  const active = data?.items.some(item => ['queued','running','waiting'].includes(item.response?.state ?? '')) ?? false;

  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);
  useEffect(() => {
    if (!enabled) return;
    const controller = new AbortController();
    const revision = settingsRevision.current;
    const writingSettings = settingsWrites.current > 0;
    setLoading(true);
    api.getConversation(caseId, undefined, controller.signal).then(next => {
      if (controller.signal.aborted) return;
      setData(current => current ? {
        ...next,
        settings: writingSettings || settingsWrites.current > 0 || revision !== settingsRevision.current ? current.settings : next.settings,
        items: merge(current.items, next.items), next_before: current.next_before,
      } : next);
      setError('');
    }).catch(reason => { if (!controller.signal.aborted) setError(errorText(reason)); })
      .finally(() => { if (!controller.signal.aborted) setLoading(false); });
    return () => controller.abort();
  }, [caseId, enabled, reload]);
  useEffect(() => {
    if (!enabled) return;
    window.addEventListener('focus', refresh);
    return () => window.removeEventListener('focus', refresh);
  }, [enabled, refresh]);
  useEffect(() => {
    if (!enabled || !active || loading) return;
    const timer = window.setTimeout(refresh, 600);
    return () => window.clearTimeout(timer);
  }, [enabled, active, loading, refresh]);

  const send = async (text: string, compact = false, settings = data?.settings, attachmentIds: string[] = []) => {
    if (pending.current || !data?.can_send || active) return false;
    if (!settings) return false;
    pending.current = true; setSending(true); setError('');
    const retryKey = `${compact ? 'compact' : 'message'}:${text}:${JSON.stringify(settings)}:${attachmentIds.join(',')}`;
    const requestId = retries.current.get(retryKey) ?? crypto.randomUUID();
    retries.current.set(retryKey, requestId);
    try {
      if (settingsWrites.current > 0 && !await settingsQueue.current) return false;
      if (!mounted.current) return false;
      const { message } = await (compact ? api.compact(caseId, requestId) : api.sendMessage(caseId, text, requestId, settings, attachmentIds));
      retries.current.delete(retryKey);
      if (!mounted.current) return false;
      setData(current => current && { ...current, items: merge(current.items, [message]) });
      refresh();
      return true;
    } catch (reason) {
      if (mounted.current) setError(errorText(reason));
      return false;
    } finally {
      pending.current = false;
      if (mounted.current) setSending(false);
    }
  };
  const updateSettings = async (settings: AgentSettings) => {
    if (!data?.can_send || active || pending.current) return false;
    settingsRevision.current++;
    settingsWrites.current++;
    setSavingSettings(true);
    const request = settingsQueue.current.then(async () => {
      try {
        const result = await api.updateConversationSettings(caseId, settings);
        if (mounted.current) {
          setData(current => current ? { ...current, settings: result.settings } : current);
          setError('');
        }
        return true;
      } catch (reason) {
        if (mounted.current) setError(errorText(reason));
        return false;
      } finally {
        settingsWrites.current--;
        if (mounted.current && settingsWrites.current === 0) setSavingSettings(false);
      }
    });
    settingsQueue.current = request.catch(() => false);
    return request;
  };
  const loadEarlier = async () => {
    if (!data?.next_before || loading) return;
    setLoading(true); setError('');
    try {
      const next = await api.getConversation(caseId, data.next_before);
      if (mounted.current) setData(current => current && { ...current, next_before: next.next_before, items: merge(current.items, next.items) });
    } catch (reason) { if (mounted.current) setError(errorText(reason)); }
    finally { if (mounted.current) setLoading(false); }
  };
  const cancel = async () => {
    const message = data?.items.find(item => ['queued','running','waiting'].includes(item.response?.state ?? ''));
    if (!message) return;
    try { await api.cancel(caseId, message.id); refresh(); } catch (reason) { if (mounted.current) setError(errorText(reason)); }
  };
  return { data, loading, sending, savingSettings, active, error, send: (text: string, settings?: AgentSettings, attachmentIds: string[] = []) => send(text, false, settings, attachmentIds), updateSettings, compact: () => send('', true), cancel, loadEarlier, refresh };
}
