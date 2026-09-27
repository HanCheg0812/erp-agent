CREATE TABLE agent_session_runtime_leases (
  id uuid PRIMARY KEY,
  session_id text NOT NULL REFERENCES agent_sessions(id) ON DELETE CASCADE,
  token_hash text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  expires_at timestamptz NOT NULL,
  revoked_at timestamptz
);
CREATE INDEX agent_session_runtime_leases_active_idx
  ON agent_session_runtime_leases(session_id, expires_at)
  WHERE revoked_at IS NULL;

ALTER TABLE case_sandboxes
  ADD COLUMN active boolean NOT NULL DEFAULT true,
  ADD COLUMN session_lease_id uuid REFERENCES agent_session_runtime_leases(id);

DROP INDEX IF EXISTS case_sandboxes_active;
CREATE UNIQUE INDEX case_sandboxes_active
  ON case_sandboxes(tenant_id, case_id)
  WHERE active AND state <> 'released';
CREATE INDEX case_sandboxes_active_lookup
  ON case_sandboxes(tenant_id, case_id, created_at DESC)
  WHERE active AND state <> 'released';
