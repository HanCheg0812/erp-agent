-- Durable document chunks and embedding work are separate from personal memory.
-- PostgreSQL remains authoritative; OpenViking is a later retrieval projection.
CREATE TABLE IF NOT EXISTS knowledge_chunks (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  tenant_id uuid NOT NULL,
  owner_id uuid NOT NULL,
  case_id uuid NOT NULL,
  document_id uuid NOT NULL,
  source_revision text NOT NULL DEFAULT 'document'
    CHECK (length(source_revision) BETWEEN 1 AND 200),
  ordinal integer NOT NULL CHECK (ordinal >= 0),
  text text NOT NULL CHECK (length(btrim(text)) BETWEEN 1 AND 12000),
  locator jsonb NOT NULL DEFAULT '{}'::jsonb,
  content_hash text NOT NULL CHECK (length(content_hash) = 64),
  state text NOT NULL DEFAULT 'ready'
    CHECK (state IN ('ready','retracted')),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (tenant_id, owner_id, case_id, id),
  UNIQUE (document_id, source_revision, ordinal, content_hash),
  FOREIGN KEY (tenant_id, owner_id, case_id, document_id)
    REFERENCES documents(tenant_id, owner_id, case_id, id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS knowledge_chunks_scope
  ON knowledge_chunks(tenant_id, owner_id, case_id, document_id, ordinal);
CREATE INDEX IF NOT EXISTS knowledge_chunks_ready
  ON knowledge_chunks(tenant_id, owner_id, case_id, created_at DESC, id)
  WHERE state = 'ready';

CREATE TABLE IF NOT EXISTS knowledge_embedding_jobs (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  tenant_id uuid NOT NULL,
  owner_id uuid NOT NULL,
  case_id uuid NOT NULL,
  chunk_id uuid NOT NULL,
  model_id text NOT NULL REFERENCES embedding_models(id),
  operation text NOT NULL CHECK (operation IN ('embed','retract')),
  content_hash text NOT NULL CHECK (length(content_hash) = 64),
  state text NOT NULL DEFAULT 'pending'
    CHECK (state IN ('pending','processing','completed','failed')),
  attempt_count integer NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
  available_at timestamptz NOT NULL DEFAULT now(),
  lease_until timestamptz,
  lease_token uuid,
  last_error text,
  created_at timestamptz NOT NULL DEFAULT now(),
  processed_at timestamptz,
  UNIQUE (chunk_id, model_id, operation, content_hash),
  FOREIGN KEY (tenant_id, owner_id, case_id, chunk_id)
    REFERENCES knowledge_chunks(tenant_id, owner_id, case_id, id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS knowledge_embedding_jobs_claim
  ON knowledge_embedding_jobs(model_id, state, available_at, created_at, id)
  WHERE state IN ('pending','processing','failed');
CREATE INDEX IF NOT EXISTS knowledge_embedding_jobs_scope
  ON knowledge_embedding_jobs(tenant_id, owner_id, case_id, created_at DESC);

-- pgvector is optional for non-vector development databases. The worker checks
-- for this table at startup and fails clearly if the vector rollout is absent.
DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM pg_available_extensions WHERE name = 'vector') THEN
    EXECUTE 'CREATE EXTENSION IF NOT EXISTS vector';
    EXECUTE $sql$
      CREATE TABLE IF NOT EXISTS knowledge_chunk_embeddings (
        chunk_id uuid NOT NULL REFERENCES knowledge_chunks(id) ON DELETE CASCADE,
        model_id text NOT NULL REFERENCES embedding_models(id),
        dimensions integer NOT NULL CHECK (dimensions > 0 AND dimensions <= 16000),
        embedding vector NOT NULL,
        embedded_text_hash text NOT NULL CHECK (length(embedded_text_hash) = 64),
        created_at timestamptz NOT NULL DEFAULT now(),
        PRIMARY KEY (chunk_id, model_id)
      )
    $sql$;
    EXECUTE 'CREATE INDEX IF NOT EXISTS knowledge_chunk_embeddings_model ON knowledge_chunk_embeddings(model_id, chunk_id)';
  END IF;
END $$;
