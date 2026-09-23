import { demoCases, demoDetail, type CaseDetail, type Proposal } from './api';

export type AgentRun = { status: 'idle' | 'running' | 'paused' | 'complete'; activeStage: number; finalized: boolean };

export const agentStages = [
  { label: 'Reading the change request', node: 'agent' },
  { label: 'Reading the order', node: 'erp' },
  { label: 'Reviewing the order result', node: 'agent' },
  { label: 'Searching applicable evidence', node: 'knowledge' },
  { label: 'Evaluating quantity and delivery impact', node: 'proposal' },
  { label: 'Preparing the approval proposal', node: 'proposal' },
] as const;

export const initialAgentRuns = Object.fromEntries(demoCases.map(item => [item.id, {
  status: item.status === 'analyzing' ? 'idle' : 'complete',
  activeStage: item.status === 'analyzing' ? 0 : agentStages.length - 1,
  finalized: item.status !== 'analyzing',
}])) as Record<string, AgentRun>;

// These fixtures simulate tool results; no ERP or knowledge service is contacted.
export function demoAgentProposal(detail: CaseDetail): Proposal | undefined {
  if (detail.id !== 'case-demo-002') return demoDetail[detail.id]?.proposals[0];
  return {
    id: 'proposal-demo-002', case_id: detail.id, case_version: 2,
    summary: 'Replace the Atlas order total with 780 units and pull delivery forward by 6 days.',
    actions: [{ type: 'revise_sales_order_item', sales_order: 'SO-AT-2026-1088', item_row: '10', expected_erp_modified: '2026-09-20T09:30:00Z', expected_quantity: 640, proposed_quantity: 780, expected_delivery_date: '2026-10-30', proposed_delivery_date: '2026-10-24' }],
    evidence: [{ document_id: '00000000-0000-4000-8000-000000000004', revision: 'R2', locator: 'page 2, delivery instruction, section 3' }],
    digest: 'd3b4f6a2c8e14d7f9a01c2e5b6d7f8091a2b3c4d5e6f708192a3b4c5d6e7f809',
    created_at: '2026-09-21T08:00:00Z',
  };
}
