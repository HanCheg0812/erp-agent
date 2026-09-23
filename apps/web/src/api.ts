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

const now = new Date();
const iso = (hoursAgo: number) => new Date(now.getTime() - hoursAgo * 60 * 60 * 1000).toISOString();

export const demoCases: CaseRecord[] = [
  { id: 'case-demo-001', title: 'Q4 Northwind delivery revision', customer_code: 'NORTHWIND', part_number: 'PM-42A', revision: 'C', demand_type: 'order', quantity_basis: 'replacement', status: 'awaiting_approval', version: 3, owner_id: 'alex.chen', created_at: iso(38), updated_at: iso(1) },
  { id: 'case-demo-002', title: 'Atlas order uplift / October', customer_code: 'ATLAS', part_number: 'PX-18', revision: 'B', demand_type: 'order', quantity_basis: 'replacement', status: 'analyzing', version: 1, owner_id: 'mira.li', created_at: iso(16), updated_at: iso(3) },
  { id: 'case-demo-003', title: 'Redwood ship date correction', customer_code: 'REDWOOD', part_number: 'RM-220', revision: 'A', demand_type: 'delivery_instruction', quantity_basis: 'replacement', status: 'approved', version: 5, owner_id: 'alex.chen', created_at: iso(74), updated_at: iso(9) },
  { id: 'case-demo-004', title: 'Cedar line item reconciliation', customer_code: 'CEDAR', part_number: 'CL-09', revision: 'D', demand_type: 'order', quantity_basis: 'unknown', status: 'rejected', version: 2, owner_id: 'nora.wu', created_at: iso(120), updated_at: iso(48) },
];

export const demoDetail: Record<string, CaseDetail> = {
  'case-demo-001': {
    ...demoCases[0],
    proposals: [{ id: 'proposal-demo-001', case_id: 'case-demo-001', case_version: 3, summary: 'Replace the Northwind order total with 1,280 units and pull delivery forward by 6 days.', actions: [{ type: 'revise_sales_order_item', sales_order: 'SO-NW-2026-1042', item_row: '10', expected_erp_modified: '2026-09-18T08:21:00Z', expected_quantity: 960, proposed_quantity: 1280, expected_delivery_date: '2026-10-28', proposed_delivery_date: '2026-10-22' }], evidence: [{ document_id: '00000000-0000-4000-8000-000000000001', revision: 'R3', locator: 'page 2, table 1' }, { document_id: '00000000-0000-4000-8000-000000000002', revision: 'R1', locator: 'email thread, 2026-09-20 14:32' }], digest: 'a92fce21c1f3c0d7c2b4b16ea2fc8014a4b6c1bbd11a8b2f8fa5e0d7b40e11aa', created_at: iso(1) }],
    approvals: [],
  },
  'case-demo-002': { ...demoCases[1], proposals: [], approvals: [] },
  'case-demo-003': { ...demoCases[2], proposals: [{ id: 'proposal-demo-003', case_id: 'case-demo-003', case_version: 5, summary: 'Move the confirmed delivery date to 2026-10-04.', actions: [{ type: 'revise_sales_order_item', sales_order: 'SO-RW-2026-0931', item_row: '20', expected_erp_modified: '2026-09-15T11:04:00Z', expected_quantity: 420, proposed_quantity: 420, expected_delivery_date: '2026-10-11', proposed_delivery_date: '2026-10-04' }], evidence: [{ document_id: '00000000-0000-4000-8000-000000000003', revision: 'R2', locator: 'page 1, delivery block' }], digest: 'b25d7b3a3b18b0d2e9314cba77eaa46de9cb6ce30e9c64c56be4c9d2c6f2a9f1', created_at: iso(9) }], approvals: [{ id: 'approval-demo-003', proposal_id: 'proposal-demo-003', proposal_digest: 'b25d7b3a3b18b0d2e9314cba77eaa46de9cb6ce30e9c64c56be4c9d2c6f2a9f1', decision: 'approved', comment: 'Matches the customer instruction and available capacity.', actor_id: 'demo-approver', created_at: iso(8) }] },
  'case-demo-004': { ...demoCases[3], proposals: [], approvals: [] },
};

export const demoEvents = (caseId: string): AuditEvent[] => {
  const item = demoDetail[caseId] ?? demoDetail['case-demo-001'];
  const events: AuditEvent[] = [{ id: '1', event_type: 'case.created', payload: { version: 1 }, actor_id: item.owner_id, created_at: item.created_at }];
  if (item.proposals[0]) events.push({ id: '2', event_type: 'proposal.created', payload: { version: item.proposals[0].case_version, proposal_id: item.proposals[0].id }, actor_id: item.owner_id, created_at: item.proposals[0].created_at });
  if (item.approvals[0]) events.push({ id: '3', event_type: `proposal.${item.approvals[0].decision}`, payload: { proposal_id: item.approvals[0].proposal_id }, actor_id: item.approvals[0].actor_id, created_at: item.approvals[0].created_at });
  return events.reverse();
};
