CREATE TABLE agent_sessions (
  id text PRIMARY KEY CHECK (id ~ '^session_[a-f0-9]{32}$'),
  tenant_id uuid NOT NULL,
  case_id uuid NOT NULL,
  owner_id uuid NOT NULL,
  revision integer NOT NULL DEFAULT 0 CHECK (revision >= 0),
  fence integer NOT NULL DEFAULT 0 CHECK (fence >= 0),
  token_hash text,
  expires_at timestamptz,
  metadata jsonb,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  FOREIGN KEY (tenant_id, case_id) REFERENCES cases(tenant_id, id),
  FOREIGN KEY (tenant_id, owner_id) REFERENCES users(tenant_id, id)
);
CREATE INDEX agent_sessions_case_idx ON agent_sessions(tenant_id, case_id, created_at DESC);
CREATE TABLE agent_session_heads (
  session_id text NOT NULL REFERENCES agent_sessions(id),
  kind text NOT NULL CHECK (kind IN ('messages','env_snapshots','memory_injections','replay_events')),
  generation integer NOT NULL,
  length integer NOT NULL CHECK (length >= 0),
  PRIMARY KEY (session_id, kind)
);
CREATE TABLE agent_session_entries (
  session_id text NOT NULL REFERENCES agent_sessions(id),
  kind text NOT NULL,
  generation integer NOT NULL,
  position integer NOT NULL CHECK (position >= 0),
  payload jsonb NOT NULL,
  PRIMARY KEY (session_id, kind, generation, position)
);
-- Replaced vectors remain available for investigation; heads select the active view.
CREATE TABLE agent_session_revisions (
  session_id text NOT NULL REFERENCES agent_sessions(id),
  revision integer NOT NULL,
  metadata jsonb NOT NULL,
  heads jsonb NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (session_id, revision)
);
CREATE TABLE agent_session_documents (
  session_id text NOT NULL REFERENCES agent_sessions(id),
  kind text NOT NULL CHECK (kind IN ('todos','goals','plan','review','gate_observations')),
  revision integer NOT NULL CHECK (revision > 0),
  payload jsonb NOT NULL,
  PRIMARY KEY (session_id, kind)
);
CREATE TABLE agent_session_receipts (
  session_id text NOT NULL REFERENCES agent_sessions(id),
  request_id uuid NOT NULL,
  digest text NOT NULL,
  result jsonb NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (session_id, request_id)
);
