import { z } from 'zod';

export const sandboxCreateSchema = z.object({ request_id: z.uuid() }).strict();
export type CaseSandbox = {
  id: string; session_id: string; sandbox_id: string | null;
  state: 'creating' | 'provisioned' | 'releasing' | 'released';
  provider_state: string | null; guest_ready: boolean; error_code: string | null;
  created_at: string; expires_at: string; checked_at: string | null;
};
export type SandboxStatus = { configured: boolean; can_manage: boolean; operation: CaseSandbox | null };

export const roleSchema = z.enum(['planner', 'approver', 'reader']);
export type Role = z.infer<typeof roleSchema>;
export const loginSchema = z.strictObject({
  tenant: z.string().regex(/^[a-z0-9-]{1,64}$/),
  email: z.email().max(254).transform((s) => s.toLowerCase()),
  password: z.string().min(1).max(256),
});
export const caseInputSchema = z.strictObject({
  title: z.string().trim().min(1).max(200),
  customer_code: z.string().trim().min(1).max(100),
  part_number: z.string().trim().min(1).max(100),
  revision: z.string().trim().min(1).max(100),
  demand_type: z.enum(['forecast', 'order', 'delivery_instruction']),
  quantity_basis: z.enum(['incremental', 'cumulative', 'replacement', 'unknown']),
});
export type CaseInput = z.infer<typeof caseInputSchema>;

// The first executable action is deliberately narrow. New ERP actions need their own schemas.
export const proposalActionSchema = z.strictObject({
  type: z.literal('revise_sales_order_item'),
  sales_order: z.string().min(1).max(140),
  item_row: z.string().min(1).max(140),
  expected_erp_modified: z.string().min(1).max(100),
  expected_quantity: z.number().int().nonnegative().max(1_000_000_000),
  proposed_quantity: z.number().int().nonnegative().max(1_000_000_000),
  expected_delivery_date: z.iso.date(),
  proposed_delivery_date: z.iso.date(),
});
export const proposalInputSchema = z.strictObject({
  expected_case_version: z.number().int().positive(),
  summary: z.string().trim().min(1).max(4000),
  actions: z.array(proposalActionSchema).min(1).max(100),
  evidence: z.array(z.strictObject({
    document_id: z.uuid(),
    revision: z.string().min(1).max(100),
    locator: z.string().min(1).max(500),
  })).min(1).max(100),
});
export type ProposalInput = z.infer<typeof proposalInputSchema>;
export const approvalInputSchema = z.strictObject({
  proposal_id: z.uuid(),
  proposal_digest: z.string().regex(/^[a-f0-9]{64}$/),
  expected_case_version: z.number().int().positive(),
  decision: z.enum(['approved', 'rejected']),
  comment: z.string().trim().max(2000).default(''),
});
export type ApprovalInput = z.infer<typeof approvalInputSchema>;
export const caseParamsSchema = z.strictObject({ caseId: z.uuid() });
export const agentSettingsSchema = z.strictObject({
  approval: z.enum(['ask', 'auto', 'full']),
  model: z.enum(['deepseek-flash', 'deepseek-v4-pro']),
  effort: z.enum(['none', 'low', 'high', 'max']),
});
export const conversationInputSchema = z.strictObject({
  client_message_id: z.uuid(),
  text: z.string().trim().max(4000),
  attachment_ids: z.array(z.uuid()).max(4).optional(),
  settings: agentSettingsSchema.optional(),
}).refine(value => !!value.text || !!value.attachment_ids?.length);
export type ConversationInput = z.infer<typeof conversationInputSchema>;
export const conversationPageSchema = z.strictObject({
  before: z.string().regex(/^[1-9][0-9]{0,18}$/).refine((s) => BigInt(s) <= 9223372036854775807n).optional(),
  limit: z.coerce.number().int().min(1).max(100).default(50),
});
export type ConversationPage = z.infer<typeof conversationPageSchema>;
export type ConversationMessage = {
  operation?: 'message' | 'compact';
  id: string; client_message_id: string; actor_id: string;
  text: string; state: 'saved'; created_at: string;
  attachments?: Array<{ id: string; filename: string; content_type: string; bytes: number; sha256: string; state: string }>;
  response?: AgentResponse | null;
};
export const agentToolNameSchema = z.enum(['memory_save', 'case_set_context', 'proposal_create', 'request_proposal_approval', 'case_read', 'todo', 'skill_find', 'skill_manage', 'mcp__erp__list_erp_orders', 'mcp__erp__read_erp_order', 'mcp__openviking__search_case_knowledge', 'mcp__openviking__read_knowledge_source']);
export type AgentToolName = z.infer<typeof agentToolNameSchema>;
export type AgentResponse = {
  state: 'queued' | 'running' | 'waiting' | 'completed' | 'failed' | 'cancelled';
  text: string;
  tools: { id: string; name: AgentToolName; state: 'running' | 'completed' | 'failed' }[];
  steps: AgentStep[];
  compactions: CompactionEvent[];
  error: string | null;
  permissions?: ToolPermission[];
};
export type ToolPermission = {
  id: string; tool_call_id: string; arguments_digest: string;
  mode: 'ask' | 'auto' | 'full'; state: 'pending' | 'allowed' | 'denied' | 'executed' | 'cancelled';
  decided_by: 'user' | 'reviewer' | 'full' | 'expiry' | 'cancel' | null; reason: string | null;
  created_at: string; expires_at: string;
  result?: { status: string } | null;
} & (
  | { tool: 'memory_save'; arguments: { text: string } }
  | { tool: 'case_set_context'; arguments: { expected_case_version: number; context: CaseInput } }
  | { tool: 'proposal_create'; arguments: ProposalInput }
  | { tool: 'request_proposal_approval'; arguments: Pick<ApprovalInput, 'proposal_id' | 'proposal_digest' | 'expected_case_version'>; proposal: Omit<ProposalInput, 'expected_case_version'> & { id: string; digest: string; case_version: number }; can_approve: boolean }
);
export type CompactionEvent = {
  trigger: 'background' | 'hard_compact' | 'auto_recovery' | string;
  pre_tokens: number | null;
  post_tokens: number | null;
  tokens_saved: number | null;
  duration_ms: number | null;
  messages_dropped: number | null;
  messages_compacted: number | null;
  summary_chars: number | null;
  active_messages: number | null;
};
export type AgentStep =
  | { id: string; kind: 'reasoning'; text: string; state: 'running' | 'completed' | 'failed' }
  | { id: string; kind: 'tool' };
export type Conversation = {
  case_version: number;
  settings: AgentSettings;
  context_usage: ContextUsage;
  config: { endpoint: string; api: string; models: string[]; efforts: string[]; context_windows: Record<string, number>; approval_modes: string[]; integrations: { erp: boolean; openviking: boolean } };
  session_id: string | null;
  items: ConversationMessage[];
  next_before: string | null;
  can_send: boolean;
  runtime: { state: 'offline'; reason: 'bridge_not_connected' } | { state: 'available' };
};
export type AgentSettings = { approval: 'ask' | 'auto' | 'full'; model: string; effort: 'none' | 'low' | 'high' | 'max' };
export type ContextUsage = {
  model: string;
  used_tokens: number;
  window_tokens: number;
  soft_ratio: number;
  critical_ratio: number;
  compacting: boolean;
  estimated: boolean;
  measured: boolean;
};
export const pageSchema = z.object({ limit: z.coerce.number().int().min(1).max(100).default(50) });
