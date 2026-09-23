CREATE TABLE approvals (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  tenant_id uuid NOT NULL,
  case_id uuid NOT NULL,
  proposal_id uuid NOT NULL,
  proposal_digest text NOT NULL,
  decision text NOT NULL CHECK (decision IN ('approved','rejected')),
  comment text NOT NULL,
  actor_id uuid NOT NULL,
  idempotency_key text NOT NULL,
  request_digest text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (tenant_id, actor_id, idempotency_key),
  UNIQUE (tenant_id, proposal_id),
  FOREIGN KEY (tenant_id, case_id, proposal_id) REFERENCES proposals(tenant_id, case_id, id),
  FOREIGN KEY (tenant_id, actor_id) REFERENCES users(tenant_id, id)
);
