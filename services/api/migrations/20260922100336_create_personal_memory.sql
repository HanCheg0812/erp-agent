CREATE TABLE personal_accounts (
  email text PRIMARY KEY CHECK(email=lower(email)),
  user_id uuid NOT NULL UNIQUE REFERENCES users(id)
);
ALTER TABLE agent_runs ADD COLUMN operation text NOT NULL DEFAULT 'message'
  CHECK(operation IN ('message','compact'));
CREATE TABLE user_memories (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(), tenant_id uuid NOT NULL,
  owner_id uuid NOT NULL, text text NOT NULL CHECK(length(text) BETWEEN 1 AND 2000),
  created_at timestamptz NOT NULL DEFAULT now(),
  FOREIGN KEY(tenant_id,owner_id) REFERENCES users(tenant_id,id)
);
CREATE TABLE documents (
  id uuid PRIMARY KEY, tenant_id uuid NOT NULL, owner_id uuid NOT NULL,
  case_id uuid NOT NULL, object_key text NOT NULL UNIQUE, filename text NOT NULL,
  content_type text NOT NULL, bytes integer NOT NULL CHECK(bytes BETWEEN 1 AND 5242880),
  sha256 text NOT NULL, state text NOT NULL CHECK(state IN ('uploading','ready','failed')),
  created_at timestamptz NOT NULL DEFAULT now(),
  FOREIGN KEY(tenant_id,owner_id) REFERENCES users(tenant_id,id),
  FOREIGN KEY(tenant_id,case_id) REFERENCES cases(tenant_id,id)
);
CREATE INDEX user_memories_owner ON user_memories(tenant_id,owner_id);
CREATE INDEX documents_case ON documents(tenant_id,owner_id,case_id);
