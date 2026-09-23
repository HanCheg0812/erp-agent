ALTER TABLE agent_runs ADD COLUMN settings jsonb NOT NULL DEFAULT '{"access":"read","model":"deepseek-flash","effort":"high"}';
ALTER TABLE agent_runs ADD COLUMN cancel_requested boolean NOT NULL DEFAULT false;
ALTER TABLE agent_runs DROP CONSTRAINT agent_runs_state_check;
ALTER TABLE agent_runs ADD CONSTRAINT agent_runs_state_check CHECK(state IN ('queued','running','completed','failed','cancelled'));
ALTER TABLE case_conversation_messages ADD COLUMN settings jsonb NOT NULL DEFAULT '{"access":"read","model":"deepseek-flash","effort":"high"}';
ALTER TABLE case_conversations ADD COLUMN settings jsonb NOT NULL DEFAULT '{"access":"read","model":"deepseek-flash","effort":"high"}';
CREATE TABLE integration_grants (
  tenant_id uuid NOT NULL, owner_id uuid NOT NULL,
  domain text NOT NULL CHECK(domain IN ('erp','openviking')),
  scope text NOT NULL CHECK(length(scope) BETWEEN 1 AND 1000),
  case_id uuid,
  FOREIGN KEY(tenant_id,owner_id) REFERENCES users(tenant_id,id),
  FOREIGN KEY(tenant_id,case_id) REFERENCES cases(tenant_id,id),
  UNIQUE NULLS NOT DISTINCT(tenant_id,owner_id,domain,scope,case_id)
);
