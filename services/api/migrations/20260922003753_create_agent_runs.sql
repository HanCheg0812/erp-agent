ALTER TABLE case_conversation_messages ADD CONSTRAINT conversation_message_scope UNIQUE (tenant_id, case_id, id);

CREATE TABLE agent_runs (
  message_id bigint PRIMARY KEY,
  tenant_id uuid NOT NULL,
  case_id uuid NOT NULL,
  state text NOT NULL DEFAULT 'queued' CHECK (state IN ('queued','running','completed','failed')),
  text text NOT NULL DEFAULT '',
  tools jsonb NOT NULL DEFAULT '[]' CHECK (jsonb_typeof(tools) = 'array'),
  error text,
  started_at timestamptz,
  finished_at timestamptz,
  updated_at timestamptz NOT NULL DEFAULT now(),
  FOREIGN KEY (tenant_id, case_id, message_id) REFERENCES case_conversation_messages(tenant_id, case_id, id)
);
CREATE UNIQUE INDEX one_active_agent_run_per_case ON agent_runs(tenant_id,case_id) WHERE state IN ('queued','running');
