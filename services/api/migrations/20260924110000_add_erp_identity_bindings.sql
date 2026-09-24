CREATE TABLE erp_connections (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  tenant_id uuid NOT NULL,
  name text NOT NULL CHECK(length(name) BETWEEN 1 AND 120),
  base_url text NOT NULL CHECK(length(base_url) BETWEEN 1 AND 500),
  auth_mode text NOT NULL CHECK(auth_mode IN ('bound_user','shared_demo')),
  active boolean NOT NULL DEFAULT true,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(tenant_id, id),
  UNIQUE(tenant_id, name),
  FOREIGN KEY(tenant_id) REFERENCES tenants(id)
);

CREATE TABLE erp_identity_bindings (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  tenant_id uuid NOT NULL,
  platform_user_id uuid NOT NULL,
  connection_id uuid NOT NULL,
  erp_user_id text NOT NULL CHECK(length(erp_user_id) BETWEEN 1 AND 200),
  credential_ref text CHECK(credential_ref IS NULL OR length(credential_ref) BETWEEN 1 AND 500),
  active boolean NOT NULL DEFAULT true,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(tenant_id, platform_user_id),
  FOREIGN KEY(tenant_id,platform_user_id) REFERENCES users(tenant_id,id),
  FOREIGN KEY(tenant_id,connection_id) REFERENCES erp_connections(tenant_id,id)
);

CREATE INDEX erp_identity_bindings_lookup
  ON erp_identity_bindings(tenant_id,platform_user_id)
  WHERE active;
