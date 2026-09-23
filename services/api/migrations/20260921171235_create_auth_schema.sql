CREATE TABLE tenants (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  slug text NOT NULL UNIQUE
);
CREATE TABLE users (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  tenant_id uuid NOT NULL REFERENCES tenants(id),
  email text NOT NULL,
  password_hash text NOT NULL,
  roles text[] NOT NULL CHECK (roles <@ ARRAY['planner','approver','reader']::text[] AND cardinality(roles) > 0),
  active boolean NOT NULL DEFAULT true,
  UNIQUE (tenant_id, email),
  UNIQUE (tenant_id, id)
);
CREATE TABLE auth_sessions (
  token_hash text PRIMARY KEY,
  tenant_id uuid NOT NULL,
  user_id uuid NOT NULL,
  expires_at timestamptz NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  FOREIGN KEY (tenant_id, user_id) REFERENCES users(tenant_id, id)
);
CREATE INDEX auth_sessions_expiry_idx ON auth_sessions(expires_at);
