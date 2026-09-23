CREATE TABLE case_sandboxes (
  id uuid PRIMARY KEY,
  tenant_id uuid NOT NULL,
  case_id uuid NOT NULL,
  session_id text NOT NULL,
  actor_id uuid NOT NULL REFERENCES users(id),
  sandbox_id text UNIQUE,
  state text NOT NULL CHECK (state IN ('creating','provisioned','releasing','released')),
  provider_state text,
  guest_ready boolean NOT NULL DEFAULT false,
  error_code text,
  created_at timestamptz NOT NULL DEFAULT now(),
  expires_at timestamptz NOT NULL DEFAULT now()+interval '15 minutes',
  checked_at timestamptz,
  FOREIGN KEY (tenant_id,case_id,session_id) REFERENCES agent_sessions(tenant_id,case_id,id)
);
CREATE UNIQUE INDEX case_sandboxes_active ON case_sandboxes(tenant_id,case_id) WHERE state <> 'released';
CREATE INDEX case_sandboxes_expiry ON case_sandboxes(expires_at) WHERE state <> 'released';
