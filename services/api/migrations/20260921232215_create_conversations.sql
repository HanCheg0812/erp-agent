ALTER TABLE agent_sessions ADD CONSTRAINT agent_sessions_case_scope UNIQUE (tenant_id, case_id, id);

CREATE TABLE case_conversations (
  tenant_id uuid NOT NULL,
  case_id uuid NOT NULL,
  session_id text NOT NULL UNIQUE,
  PRIMARY KEY (tenant_id, case_id),
  FOREIGN KEY (tenant_id, case_id, session_id) REFERENCES agent_sessions(tenant_id, case_id, id)
);

-- Durable user inputs are an inbox for a future JCode bridge, not model responses.
-- The bridge must explicitly dispatch them; saving does not start execution.
CREATE TABLE case_conversation_messages (
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  tenant_id uuid NOT NULL,
  case_id uuid NOT NULL,
  actor_id uuid NOT NULL,
  client_message_id uuid NOT NULL,
  text text NOT NULL CHECK (length(text) BETWEEN 1 AND 4000),
  state text NOT NULL DEFAULT 'saved' CHECK (state = 'saved'),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (tenant_id, case_id, client_message_id),
  FOREIGN KEY (tenant_id, case_id) REFERENCES case_conversations(tenant_id, case_id),
  FOREIGN KEY (tenant_id, actor_id) REFERENCES users(tenant_id, id)
);
CREATE INDEX case_conversation_history_idx ON case_conversation_messages(tenant_id, case_id, id DESC);
