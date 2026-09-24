ALTER TABLE integration_grants
  ADD COLUMN tool_id text CHECK (tool_id IS NULL OR length(tool_id) BETWEEN 1 AND 200),
  ADD COLUMN action text NOT NULL DEFAULT 'read' CHECK (action IN ('read','simulate','prepare','write')),
  ADD COLUMN resource_type text NOT NULL DEFAULT 'legacy_scope' CHECK (length(resource_type) BETWEEN 1 AND 100),
  ADD COLUMN effect text NOT NULL DEFAULT 'allow' CHECK (effect IN ('allow','deny')),
  ADD COLUMN policy_revision bigint NOT NULL DEFAULT 1 CHECK (policy_revision > 0),
  ADD COLUMN active boolean NOT NULL DEFAULT true;

ALTER TABLE integration_grants
  DROP CONSTRAINT IF EXISTS integration_grants_tenant_id_owner_id_domain_scope_case_id_key;

ALTER TABLE integration_grants
  ADD CONSTRAINT integration_grants_policy_key
  UNIQUE NULLS NOT DISTINCT(tenant_id,owner_id,domain,scope,case_id,tool_id,action,effect);

CREATE INDEX integration_grants_tool_lookup
  ON integration_grants(tenant_id,owner_id,domain,case_id,tool_id,action)
  WHERE active;
