/// <reference types="vite/client" />
import type { AgentSettings, Conversation, ConversationMessage } from '../../../packages/contracts/src/index';
export type { Conversation, ConversationMessage } from '../../../packages/contracts/src/index';

export type CaseStatus = 'analyzing' | 'awaiting_approval' | 'approved' | 'rejected';

export type CaseRecord = {
  id: string;
  title: string;
  kind?: 'case' | 'conversation';
  customer_code: string | null;
  part_number: string | null;
  revision: string | null;
  demand_type: 'forecast' | 'order' | 'delivery_instruction' | null;
  quantity_basis: 'incremental' | 'cumulative' | 'replacement' | 'unknown';
  status: CaseStatus;
  version: number;
  owner_id: string;
  created_at: string;
  updated_at: string;
};

export type CaseInput = {
  title: string; customer_code: string; part_number: string; revision: string;
  demand_type: NonNullable<CaseRecord['demand_type']>; quantity_basis: CaseRecord['quantity_basis'];
};

export type ProposalAction = {
  type: 'revise_sales_order_item';
  sales_order: string;
  item_row: string;
  expected_erp_modified: string;
  expected_quantity: number;
  proposed_quantity: number;
  expected_delivery_date: string;
  proposed_delivery_date: string;
};

export type Proposal = {
  id: string;
  case_id: string;
  case_version: number;
  summary: string;
  actions: ProposalAction[];
  evidence: Array<{ document_id: string; revision: string; locator: string }>;
  digest: string;
  created_at: string;
};

export type Approval = {
  id: string;
  proposal_id: string;
  proposal_digest: string;
  decision: 'approved' | 'rejected';
  comment: string;
  actor_id: string;
  created_at: string;
};

export type CaseDetail = CaseRecord & { proposals: Proposal[]; approvals: Approval[] };

export type AuditEvent = {
  id: string;
  event_type: string;
  payload: Record<string, unknown>;
  actor_id: string;
  created_at: string;
};

const API_URL = ((import.meta.env.VITE_API_URL as string | undefined) ?? '/v1').replace(/\/$/, '');
const TOKEN_KEY = 'erp-agent-token';
const AUTH_EVENT = 'erp-agent-auth-change';

export const getToken = () => window.localStorage.getItem(TOKEN_KEY);
export const setToken = (token: string) => {
  window.localStorage.setItem(TOKEN_KEY, token);
  window.dispatchEvent(new CustomEvent(AUTH_EVENT, { detail: '' }));
};
export const clearToken = (notice = '') => {
  window.localStorage.removeItem(TOKEN_KEY);
  window.dispatchEvent(new CustomEvent(AUTH_EVENT, { detail: notice }));
};
export function subscribeAuthChange(listener: (notice: string) => void) {
  const changed = (event: Event) => listener((event as CustomEvent<string>).detail);
  const stored = (event: StorageEvent) => {
    if (event.storageArea === window.localStorage && (event.key === TOKEN_KEY || event.key === null)) listener('');
  };
  window.addEventListener(AUTH_EVENT, changed);
  window.addEventListener('storage', stored);
  return () => {
    window.removeEventListener(AUTH_EVENT, changed);
    window.removeEventListener('storage', stored);
  };
}

async function fetchApi(path: string, init: RequestInit = {}) {
  const authenticated = path !== '/auth/login' && path !== '/auth/register';
  const token = authenticated ? getToken() : null;
  const response = await fetch(`${API_URL}${path}`, {
    ...init,
    headers: {
      ...(init.body === undefined ? {} : { 'Content-Type': 'application/json' }),
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
      ...(init.headers ?? {}),
    },
  });
  // A response from the previous account must not affect a new session.
  if (authenticated && token !== getToken()) throw new Error('Your session changed. Please try again.');
  if (authenticated && token && response.status === 401) clearToken('Your session has expired. Sign in again.');
  return response;
}

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const response = await fetchApi(path, init);
  if (!response.ok) {
    const error = await response.json().catch(() => null) as { message?: string } | null;
    if (path === '/auth/login' && response.status === 401) throw new Error('Email or password is incorrect.');
    throw new Error(error?.message ?? `Request failed (${response.status})`);
  }
  if (response.status === 204) return undefined as T;
  return response.json() as Promise<T>;
}

export async function login(input: { email: string; password: string }, register = false) {
  const result = await request<{ access_token: string }>(register ? '/auth/register' : '/auth/login', { method: 'POST', body: JSON.stringify(input) });
  setToken(result.access_token);
  return result;
}

export const api = {
  me: () => request<{ userId: string; roles: string[]; email: string }>('/auth/me'),
  memories: () => request<{ items: PersonalMemory[] }>('/memory'),
  saveMemory: (text: string, request_id: string) => request<PersonalMemory>('/memory', { method: 'POST', body: JSON.stringify({ text, request_id }) }),
  deleteMemory: (id: string) => request<void>(`/memory/${id}`, { method: 'DELETE' }),
  documents: (id: string) => request<{ items: CaseDocument[]; configured: boolean }>(`/cases/${id}/documents`),
  uploadDocument: (id: string, input: { request_id: string; filename: string; content_type: string; data_base64: string }) => request<CaseDocument>(`/cases/${id}/documents`, { method: 'POST', body: JSON.stringify(input) }),
  downloadDocument: async (document: Pick<CaseDocument, 'id' | 'filename'>) => {
    const response = await fetchApi(`/documents/${document.id}`);
    if (!response.ok) throw new Error('Document could not be downloaded.');
    const url = URL.createObjectURL(await response.blob());
    const anchor = window.document.createElement('a'); anchor.href = url; anchor.download = document.filename; anchor.click();
    window.setTimeout(() => URL.revokeObjectURL(url), 1000);
  },
  logout: () => request<void>('/auth/logout', { method: 'POST' }),
  listCases: () => request<{ items: CaseRecord[] }>('/cases'),
  getCase: (id: string) => request<CaseDetail>(`/cases/${id}`),
  getEvents: (id: string) => request<{ items: AuditEvent[] }>(`/cases/${id}/events?limit=50&after=0`),
  getConversation: (id: string, before?: string, signal?: AbortSignal) => request<Conversation>(`/cases/${id}/conversation${before ? `?before=${encodeURIComponent(before)}` : ''}`, { signal }),
  updateConversationSettings: (id: string, settings: AgentSettings) => request<{ settings: AgentSettings }>(`/cases/${id}/conversation/settings`, {
    method: 'PATCH', body: JSON.stringify(settings),
  }),
  sendMessage: (id: string, text: string, client_message_id: string, settings: AgentSettings, attachment_ids: string[] = []) => request<{ message: ConversationMessage; replayed: boolean }>(`/cases/${id}/conversation/messages`, {
    method: 'POST', body: JSON.stringify({ text, client_message_id, settings, attachment_ids }),
  }),
  cancel: (id: string, message_id: string) => request<{ state: string }>(`/cases/${id}/conversation/cancel`, { method: 'POST', body: JSON.stringify({ message_id }) }),
  decideTool: (id: string, permission: string, decision: 'allowed' | 'denied', arguments_digest: string) => request<{ state: string }>(`/cases/${id}/tool-permissions/${permission}`, { method: 'POST', body: JSON.stringify({ decision, arguments_digest }) }),
  compact: (id: string, request_id: string) => request<{ message: ConversationMessage }>(`/cases/${id}/compactions`, { method: 'POST', body: JSON.stringify({ request_id }) }),
  startConversation: (request_id: string) => request<CaseRecord>('/conversations', { method: 'POST', body: JSON.stringify({ request_id }) }),
  setCaseContext: (id: string, context: CaseInput, expected_case_version: number) => request<CaseRecord>(`/cases/${id}/context`, { method: 'PATCH', body: JSON.stringify({ context, expected_case_version }) }),
  createCase: (input: CaseInput) =>
    request<CaseRecord>('/cases', { method: 'POST', body: JSON.stringify(input) }),
  decide: (id: string, input: { proposal_id: string; proposal_digest: string; expected_case_version: number; decision: 'approved' | 'rejected'; comment: string }, retryKey: string) =>
    request<{ approval: Approval }>(`/cases/${id}/approvals`, {
      method: 'POST',
      headers: { 'Idempotency-Key': retryKey },
      body: JSON.stringify(input),
    }),
};

export type PersonalMemory = { id: string; text: string; created_at: string };
export type CaseDocument = { id: string; filename: string; content_type: string; bytes: number; state: 'uploading' | 'ready' | 'failed'; created_at: string };
