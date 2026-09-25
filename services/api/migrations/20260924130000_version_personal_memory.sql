-- Personal memory remains in PostgreSQL, but each row is now an immutable
-- version with an explicit lifecycle. The existing table name is retained so
-- older clients and the first demo fixtures continue to work.
CREATE EXTENSION IF NOT EXISTS pg_trgm;

ALTER TABLE user_memories
  ADD COLUMN IF NOT EXISTS memory_id uuid DEFAULT gen_random_uuid(),
  ADD COLUMN IF NOT EXISTS version integer NOT NULL DEFAULT 1,
  ADD COLUMN IF NOT EXISTS scope_kind text NOT NULL DEFAULT 'user',
  ADD COLUMN IF NOT EXISTS scope_id uuid,
  ADD COLUMN IF NOT EXISTS category text NOT NULL DEFAULT 'preference',
  ADD COLUMN IF NOT EXISTS status text NOT NULL DEFAULT 'active',
  ADD COLUMN IF NOT EXISTS supersedes uuid,
  ADD COLUMN IF NOT EXISTS valid_from timestamptz NOT NULL DEFAULT now(),
  ADD COLUMN IF NOT EXISTS valid_to timestamptz,
  ADD COLUMN IF NOT EXISTS review_after timestamptz,
  ADD COLUMN IF NOT EXISTS source_kind text NOT NULL DEFAULT 'api',
  ADD COLUMN IF NOT EXISTS source_id uuid,
  ADD COLUMN IF NOT EXISTS source_hash text,
  ADD COLUMN IF NOT EXISTS approved_by uuid,
  ADD COLUMN IF NOT EXISTS approved_at timestamptz,
  ADD COLUMN IF NOT EXISTS approval_hash text,
  ADD COLUMN IF NOT EXISTS normalized_text text
    GENERATED ALWAYS AS (lower(btrim(text))) STORED,
  ADD COLUMN IF NOT EXISTS search_vector tsvector
    GENERATED ALWAYS AS (to_tsvector('simple', text)) STORED,
  ADD COLUMN IF NOT EXISTS updated_at timestamptz NOT NULL DEFAULT now();

UPDATE user_memories
SET memory_id = id,
    scope_id = owner_id,
    updated_at = COALESCE(updated_at, created_at),
    valid_from = COALESCE(valid_from, created_at)
WHERE memory_id IS NULL OR scope_id IS NULL;

ALTER TABLE user_memories
  ALTER COLUMN memory_id SET NOT NULL;

ALTER TABLE user_memories
  DROP CONSTRAINT IF EXISTS user_memories_scope_kind_check,
  DROP CONSTRAINT IF EXISTS user_memories_category_check,
  DROP CONSTRAINT IF EXISTS user_memories_status_check,
  ADD CONSTRAINT user_memories_scope_kind_check CHECK (scope_kind IN ('user','customer','tenant')),
  ADD CONSTRAINT user_memories_category_check CHECK (category IN ('preference','fact','procedure','correction')),
  ADD CONSTRAINT user_memories_status_check CHECK (status IN ('proposed','active','superseded','rejected','retracted')),
  ADD CONSTRAINT user_memories_version_check CHECK (version > 0);

ALTER TABLE user_memories
  ADD CONSTRAINT user_memories_supersedes_fk
    FOREIGN KEY (supersedes) REFERENCES user_memories(id);

CREATE INDEX IF NOT EXISTS user_memories_active_scope
  ON user_memories(tenant_id, scope_kind, scope_id, valid_from DESC, id DESC)
  WHERE status = 'active';
CREATE INDEX IF NOT EXISTS user_memories_active_owner
  ON user_memories(tenant_id, owner_id, created_at DESC, id DESC)
  WHERE status = 'active';
CREATE INDEX IF NOT EXISTS user_memories_text_trgm
  ON user_memories USING gin (text gin_trgm_ops)
  WHERE status = 'active';
CREATE INDEX IF NOT EXISTS user_memories_search_vector
  ON user_memories USING gin (search_vector)
  WHERE status = 'active';

CREATE TABLE IF NOT EXISTS embedding_models (
  id text PRIMARY KEY,
  provider text NOT NULL,
  model_name text NOT NULL,
  dimensions integer NOT NULL CHECK (dimensions > 0 AND dimensions <= 16000),
  metric text NOT NULL CHECK (metric IN ('cosine','l2','ip')),
  storage_type text NOT NULL CHECK (storage_type IN ('vector','halfvec','bit','sparsevec')),
  normalized boolean NOT NULL DEFAULT false,
  active boolean NOT NULL DEFAULT true,
  created_at timestamptz NOT NULL DEFAULT now()
);

-- The dimension is intentionally not fixed until an embedding model is
-- selected. A model-specific HNSW/IVFFlat index is added in that model's
-- rollout migration after the measured dimension is known.
CREATE TABLE IF NOT EXISTS memory_outbox (
  event_id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  tenant_id uuid NOT NULL,
  owner_id uuid NOT NULL,
  memory_version_id uuid NOT NULL REFERENCES user_memories(id) ON DELETE CASCADE,
  operation text NOT NULL CHECK (operation IN ('embed','project','retract')),
  attempt_count integer NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
  state text NOT NULL DEFAULT 'pending' CHECK (state IN ('pending','processing','completed','failed')),
  last_error text,
  created_at timestamptz NOT NULL DEFAULT now(),
  processed_at timestamptz,
  UNIQUE (memory_version_id, operation)
);
CREATE INDEX IF NOT EXISTS memory_outbox_pending
  ON memory_outbox(state, created_at, event_id)
  WHERE state IN ('pending','processing','failed');

-- pgvector is an optional rollout dependency. Keep the authoritative memory
-- state and durable embedding work queue usable on the stock local image; the
-- vector table is created when the extension is available, before the vector
-- retrieval worker is enabled.
DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM pg_available_extensions WHERE name = 'vector') THEN
    EXECUTE 'CREATE EXTENSION IF NOT EXISTS vector';
    EXECUTE $sql$
      CREATE TABLE IF NOT EXISTS memory_embeddings (
        memory_version_id uuid NOT NULL REFERENCES user_memories(id) ON DELETE CASCADE,
        model_id text NOT NULL REFERENCES embedding_models(id),
        dimensions integer NOT NULL CHECK (dimensions > 0 AND dimensions <= 16000),
        embedding vector NOT NULL,
        embedded_text_hash text NOT NULL,
        created_at timestamptz NOT NULL DEFAULT now(),
        PRIMARY KEY (memory_version_id, model_id)
      )
    $sql$;
  END IF;
END $$;
