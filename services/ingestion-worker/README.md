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

Still to implement: real parser adapters, normalized evidence/coordinates, durable
jobs and artifact storage, queue processing, indexing and an executable worker.
The structs are internal Rust contracts; no cross-language API schema is published.

Target flow: original storage -> durable job/outbox -> queue -> parsing -> validation -> indexing -> ready.

Use job IDs, leases/heartbeats, bounded retries, idempotency, phase checkpoints, cancellation, and explicit failed/needs-review states. Separate CPU parsing, GPU parsing, and indexing concurrency. Publish a document revision only after successful validation and indexing.

See [the pipeline design](../../docs/plantree/plans/server-demo/parser-pipeline-design.md).
