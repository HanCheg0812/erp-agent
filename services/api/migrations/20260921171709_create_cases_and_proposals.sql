CREATE TABLE cases (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  tenant_id uuid NOT NULL REFERENCES tenants(id),
  owner_id uuid NOT NULL,
  title text NOT NULL,
  customer_code text NOT NULL,
  part_number text NOT NULL,
  revision text NOT NULL,
  demand_type text NOT NULL CHECK (demand_type IN ('forecast','order','delivery_instruction')),
  quantity_basis text NOT NULL CHECK (quantity_basis IN ('incremental','cumulative','replacement','unknown')),
  status text NOT NULL DEFAULT 'analyzing' CHECK (status IN ('analyzing','awaiting_approval','approved','rejected')),
  version integer NOT NULL DEFAULT 1 CHECK (version > 0),
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (tenant_id, id),
  FOREIGN KEY (tenant_id, owner_id) REFERENCES users(tenant_id, id)
);
CREATE INDEX cases_tenant_recent_idx ON cases(tenant_id, created_at DESC, id DESC);
CREATE TABLE proposals (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  tenant_id uuid NOT NULL,
  case_id uuid NOT NULL,
  case_version integer NOT NULL,
  author_id uuid NOT NULL,
  summary text NOT NULL,
  actions jsonb NOT NULL CHECK (jsonb_typeof(actions) = 'array' AND jsonb_array_length(actions) > 0),
  evidence jsonb NOT NULL CHECK (jsonb_typeof(evidence) = 'array' AND jsonb_array_length(evidence) > 0),
  digest text NOT NULL CHECK (digest ~ '^[a-f0-9]{64}$'),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (tenant_id, case_id, case_version),
  UNIQUE (tenant_id, case_id, id),
  FOREIGN KEY (tenant_id, case_id) REFERENCES cases(tenant_id, id),
  FOREIGN KEY (tenant_id, author_id) REFERENCES users(tenant_id, id)
);
