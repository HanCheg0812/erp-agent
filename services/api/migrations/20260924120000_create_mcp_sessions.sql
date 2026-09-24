ALTER TABLE audit_events ALTER COLUMN case_id DROP NOT NULL;

CREATE TABLE mcp_sessions (
  id text PRIMARY KEY CHECK (id ~ '^mcp_[a-f0-9]{32}$'),
  tenant_id uuid NOT NULL,
  owner_id uuid NOT NULL,
  case_id uuid,
  run_message_id bigint,
  token_hash text NOT NULL,
  expires_at timestamptz NOT NULL,
  revoked_at timestamptz,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  FOREIGN KEY (tenant_id, owner_id) REFERENCES users(tenant_id, id),
  FOREIGN KEY (tenant_id, case_id) REFERENCES cases(tenant_id, id),
  FOREIGN KEY (run_message_id) REFERENCES agent_runs(message_id)
);
CREATE INDEX mcp_sessions_owner_idx ON mcp_sessions(tenant_id, owner_id, created_at DESC);
CREATE INDEX mcp_sessions_active_idx ON mcp_sessions(id, expires_at) WHERE revoked_at IS NULL;
CREATE UNIQUE INDEX mcp_sessions_active_run_idx ON mcp_sessions(run_message_id) WHERE run_message_id IS NOT NULL AND revoked_at IS NULL;
