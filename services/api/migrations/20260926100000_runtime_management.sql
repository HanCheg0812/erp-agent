ALTER TABLE users DROP CONSTRAINT users_roles_check;
ALTER TABLE users ADD CONSTRAINT users_roles_check
  CHECK (roles <@ ARRAY['planner','approver','reader','runtime_admin']::text[] AND cardinality(roles) > 0);

CREATE TABLE runtime_candidates (
  id uuid PRIMARY KEY,
  name text NOT NULL CHECK (char_length(name) BETWEEN 3 AND 80),
  image text NOT NULL,
  cpu_milli integer NOT NULL CHECK (cpu_milli BETWEEN 250 AND 2000),
  memory_mib integer NOT NULL CHECK (memory_mib BETWEEN 512 AND 4096),
  state text NOT NULL CHECK (state IN ('draft','requested','building','ready','failed','unconfirmed')),
  source text NOT NULL CHECK (source IN ('build','adopt')),
  template_id text UNIQUE,
  job_id text,
  error_code text,
  created_by uuid NOT NULL REFERENCES users(id),
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE runtime_releases (
  id uuid PRIMARY KEY,
  candidate_id uuid NOT NULL REFERENCES runtime_candidates(id),
  template_id text NOT NULL,
  activated_by uuid NOT NULL REFERENCES users(id),
  created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE runtime_control (
  singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
  active_release_id uuid REFERENCES runtime_releases(id),
  version bigint NOT NULL DEFAULT 0
);
INSERT INTO runtime_control(singleton) VALUES(true);

CREATE TABLE runtime_canaries (
  candidate_id uuid PRIMARY KEY REFERENCES runtime_candidates(id),
  tenant_id uuid NOT NULL,
  case_id uuid NOT NULL,
  actor_id uuid NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (tenant_id, case_id),
  FOREIGN KEY (tenant_id, case_id) REFERENCES cases(tenant_id, id),
  FOREIGN KEY (tenant_id, actor_id) REFERENCES users(tenant_id, id)
);

ALTER TABLE case_sandboxes
  ADD COLUMN runtime_template_id text,
  ADD COLUMN runtime_candidate_id uuid REFERENCES runtime_candidates(id),
  ADD COLUMN runtime_release_id uuid REFERENCES runtime_releases(id),
  ADD COLUMN last_sandbox_id text;
CREATE INDEX case_sandboxes_runtime_template_idx ON case_sandboxes(runtime_template_id, created_at DESC);
