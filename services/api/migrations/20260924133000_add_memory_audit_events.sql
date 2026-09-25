CREATE TABLE memory_audit_events (
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  tenant_id uuid NOT NULL,
  owner_id uuid NOT NULL,
  memory_version_id uuid NOT NULL REFERENCES user_memories(id) ON DELETE CASCADE,
  event_type text NOT NULL CHECK (event_type IN ('memory.created','memory.retracted','memory.superseded')),
  payload jsonb NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  FOREIGN KEY (tenant_id,owner_id) REFERENCES users(tenant_id,id)
);
CREATE INDEX memory_audit_owner_order_idx
  ON memory_audit_events(tenant_id,owner_id,created_at DESC,id DESC);
CREATE INDEX memory_audit_version_idx
  ON memory_audit_events(memory_version_id,created_at DESC,id DESC);
