CREATE TABLE cube_quantity_tool_calls (
  id uuid PRIMARY KEY,
  tenant_id uuid NOT NULL,
  case_id uuid NOT NULL,
  session_id text NOT NULL REFERENCES agent_sessions(id),
  message_id bigint NOT NULL REFERENCES agent_runs(message_id),
  tool_call_id text NOT NULL,
  input_digest text NOT NULL,
  template_id text NOT NULL,
  sandbox_id text,
  state text NOT NULL CHECK (state IN ('creating','running','completed','failed')),
  result jsonb,
  error_code text,
  cleanup_confirmed boolean NOT NULL DEFAULT false,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (session_id, tool_call_id),
  FOREIGN KEY (tenant_id, case_id, message_id)
    REFERENCES case_conversation_messages(tenant_id, case_id, id)
);
CREATE INDEX cube_quantity_tool_calls_cleanup_idx
  ON cube_quantity_tool_calls(created_at)
  WHERE state IN ('creating','running') OR NOT cleanup_confirmed;
