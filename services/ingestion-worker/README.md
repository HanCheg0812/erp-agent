# Ingestion Worker

Rust library foundation for asynchronous ingestion outside case sandboxes.
Run `npm run parser:test` from the repository root.

Implemented:

- `ParseBackend`: capabilities, model/option validation, submit/poll/cancel,
  typed failures and serializable remote handles.
- Versioned single-parent profiles with abstract bases, strict fields, cycle
  detection, recursive option merging and array replacement. Switching a backend
  resets its model/options. Original retention cannot be disabled.
- Extract/vision/validate dependencies, with final validation required to depend
  on every enabled parsing step. Canonical resolved JSON and SHA-256 snapshots.
- Tenant/revision/input/profile/step/replicate identity for deterministic retry
  keys and independent experiments.

Five fixture tests cover configuration boundaries, dependency order, switching
backends, stable hashes, experiment isolation and remote-handle serialization.
They do not contact MinerU or a vision model and do not prove worker recovery.

The crate now also contains the first RAG embedding slice:

- `embedding::TeiClient` calls the Hugging Face Text Embeddings Inference HTTP
  API (`/health`, `/info`, `/embed`) with bounded input and response validation.
- `embedding-worker` claims `knowledge_embedding_jobs` with PostgreSQL leases,
  retries failed calls with backoff, checks the SHA-256 text hash and registered
  model dimension, then upserts `knowledge_chunk_embeddings` in pgvector.
- The worker never consumes `memory_outbox`; personal memory remains PostgreSQL
  authoritative and uses its existing lexical recall path until a separate
  policy decision enables semantic memory retrieval.

Personal memory embedding is disabled by default. Set
`PERSONAL_MEMORY_EMBEDDINGS_ENABLED=1` only after selecting and validating a
separate memory model and retrieval policy; this does not make the RAG worker
consume `memory_outbox`.

TEI runs as a local Docker service, outside per-case sandboxes. The default
Compose profile uses the CPU image so it works without the NVIDIA Container
Toolkit. It pins the BGE-M3 model revision and persists its download cache in
`tei-model-cache`. The GPU override selects TEI's Blackwell 12.0 image; it
requires the NVIDIA Container Toolkit. Both variants bind port 8080 to
`127.0.0.1` only.

```bash
npm run dev:init
npm run embedding:up
# Or, after `docker run --rm --gpus all busybox:1.36 true` succeeds:
npm run embedding:up:gpu
curl -fsS http://127.0.0.1:8080/health
```

Run the worker after applying migrations and registering the model:

```sql
INSERT INTO embedding_models
  (id, provider, model_name, dimensions, metric, storage_type, normalized)
VALUES
  ('bge-m3', 'tei', 'BAAI/bge-m3', 1024, 'cosine', 'vector', true)
ON CONFLICT (id) DO UPDATE
SET model_name = EXCLUDED.model_name,
    dimensions = EXCLUDED.dimensions,
    active = true;
```

```bash
npm run dev:embedding-worker
```

The worker reads `.env` from the project root, checks TEI `/health` and `/info`,
and refuses to start if the served model differs from the registered model.

`RAG_EMBEDDING_BATCH_SIZE`, `RAG_EMBEDDING_LEASE_SECONDS`,
`RAG_EMBEDDING_POLL_MS` and `RAG_EMBEDDING_TIMEOUT_SECONDS` tune the worker;
`TEI_API_KEY` is optional. Document parsing still needs to create normalized
`knowledge_chunks` and enqueue jobs, and OpenViking publication remains the
next adapter. The first worker batches by item count; token-budget batching and
retrieval indexes belong to the next performance pass.

Target flow: original storage -> durable job/outbox -> queue -> parsing -> validation -> indexing -> ready.

Use job IDs, leases/heartbeats, bounded retries, idempotency, phase checkpoints, cancellation, and explicit failed/needs-review states. Separate CPU parsing, GPU parsing, and indexing concurrency. Publish a document revision only after successful validation and indexing.

See [the pipeline design](../../docs/plantree/plans/server-demo/parser-pipeline-design.md).
