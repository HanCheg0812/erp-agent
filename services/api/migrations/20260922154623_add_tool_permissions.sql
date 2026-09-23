ALTER TABLE agent_runs DROP CONSTRAINT agent_runs_state_check;
ALTER TABLE agent_runs ADD CONSTRAINT agent_runs_state_check CHECK(state IN ('queued','running','waiting','completed','failed','cancelled'));
ALTER TABLE agent_runs ADD COLUMN resumable boolean NOT NULL DEFAULT false;
DROP INDEX one_active_agent_run_per_case;
CREATE UNIQUE INDEX one_active_agent_run_per_case ON agent_runs(tenant_id,case_id) WHERE state IN ('queued','running','waiting');

UPDATE agent_runs SET settings=(settings-'access')||'{"approval":"ask"}';
UPDATE case_conversation_messages SET settings=(settings-'access')||'{"approval":"ask"}';
UPDATE case_conversations SET settings=(settings-'access')||'{"approval":"ask"}';
ALTER TABLE agent_runs ALTER COLUMN settings SET DEFAULT '{"approval":"ask","model":"deepseek-flash","effort":"high"}';
ALTER TABLE case_conversation_messages ALTER COLUMN settings SET DEFAULT '{"approval":"ask","model":"deepseek-flash","effort":"high"}';
ALTER TABLE case_conversations ALTER COLUMN settings SET DEFAULT '{"approval":"ask","model":"deepseek-flash","effort":"high"}';

CREATE TABLE agent_tool_permissions (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  tenant_id uuid NOT NULL, owner_id uuid NOT NULL, case_id uuid NOT NULL,
  message_id bigint NOT NULL, session_id text NOT NULL REFERENCES agent_sessions(id),
  tool_call_id text NOT NULL CHECK(length(tool_call_id) BETWEEN 1 AND 256),
  tool text NOT NULL CHECK(tool IN ('memory_save')),
  arguments jsonb NOT NULL, arguments_digest text NOT NULL,
  mode text NOT NULL CHECK(mode IN ('ask','auto','full')),
  state text NOT NULL CHECK(state IN ('pending','allowed','denied','executed','cancelled')),
  decided_by text CHECK(decided_by IN ('user','reviewer','full','expiry','cancel')),
  reason text, result jsonb,
  created_at timestamptz NOT NULL DEFAULT now(),
  expires_at timestamptz NOT NULL DEFAULT now()+interval '24 hours',
  decided_at timestamptz, executed_at timestamptz,
  UNIQUE(session_id,tool_call_id),
  FOREIGN KEY(tenant_id,owner_id) REFERENCES users(tenant_id,id),
  FOREIGN KEY(tenant_id,case_id,message_id) REFERENCES case_conversation_messages(tenant_id,case_id,id)
);
CREATE INDEX pending_agent_tool_permissions ON agent_tool_permissions(message_id) WHERE state='pending';
