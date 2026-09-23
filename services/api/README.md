# Rust Business API

The deployable business backend is Rust/Axum. `src/lib.rs` owns the reusable service router and lifecycle, while `src/main.rs` is only the executable entrypoint. PostgreSQL is the durable store for identity, cases, proposals, approvals, audit, conversation messages, JCode state, compaction checkpoints and personal memory. RustFS stores private original uploads; originals are not called indexed until a future parsing/indexing worker publishes a knowledge artifact.

Run `npm run db:migrate`, `npm run db:seed`, `npm run storage:init`, then `npm run dev:api`. The service listens on `127.0.0.1:3100`; `/health` is process health and `/ready` verifies every immutable migration checksum.

SQL migrations use `YYYYMMDDHHmmss_<change_description>.sql` (UTC), rather than sequential counters. The initial migration history has been renamed to this convention; see [migration naming and registration](migrations/README.md).

Rust modules are directory-driven: each bounded area lives under `src/<area>/mod.rs` (`cases`, `auth`, `conversation`, `agent_sessions`, `sandboxes`, `integrations`, `objects`, `permissions`, `platform`, and `worker`). Keep HTTP route wiring and lifecycle composition in `src/lib.rs`; keep `src/main.rs` as a thin executable entrypoint. Add domain behavior inside the owning directory instead of creating new root-level function files.

`mod.rs` only declares modules and explicit exports. `handlers.rs` only adapts HTTP inputs, credentials and responses; it does not open transactions, query PostgreSQL, call providers or make business decisions. These boundaries have source-level regression checks in `src/architecture_tests.rs`.

The case workflow separates responsibilities explicitly:

```text
cases/
  handlers.rs        # HTTP extraction and status/JSON mapping
  types.rs           # Case, proposal and approval input types
  domain.rs          # Validation, transitions and approval intent digest
  application/       # Named use cases, authorization and transaction ownership
  repository/        # Scoped SQL, locking, persistence and transaction audit
  mod.rs             # Module declarations and selected exports only
```

`conversation`, `permissions` and `objects` now also use `application/`, `domain.rs` and `repository/` boundaries. Repositories are grouped by messages/runs/queries/settings, permission actions/decisions/recovery/session, and documents/memory respectively; they receive a connection or transaction and never commit independently. Conversation operation/replay/cancellation rules, permission binding/decision/expiry rules and original-upload validation have database-free tests. Agent memory writes join the permission transaction through the objects application API, rather than modifying memory tables from permissions. RustFS network I/O remains in `objects/storage.rs`.

Other areas separate HTTP from named responsibilities: auth accounts/passwords/sessions; agent session registration/leases/storage/documents/receipts; integrations gateway/ERP/knowledge/response; and sandboxes lifecycle/provider/reconciliation/repository/sweeper. This is not yet a fully persistence-independent domain model: auth, agent session, integration and sandbox use cases still contain some SQL, and rules still return shared API error types. Do not concentrate new behavior in a generic `service.rs` or claim that directories alone complete DDD. See the [boundary refactor evidence](../../docs/plantree/plans/server-demo/api-module-boundaries.md).

Authentication is email/password registration and bearer login. Registration creates a personal tenant and planner role. Passwords use Argon2id; old local scrypt hashes are upgraded on successful login. There is no email verification, reset mail, SSO or production account lifecycle yet.

The authenticated API covers cases, exact-version proposals and approvals, audit events, paged conversations, personal memory, private document upload/download, Cube sandbox lifecycle, and scoped JCode storage. Model input is queued in PostgreSQL and the worker starts a bounded Rust JCode process with only `case_read`, `todo`, `skill_find` and read-only `skill_manage`. Every runtime gets an ephemeral HOME and a case-bound storage token; no upstream JSON/JSONL session files are retained.

`POST /v1/cases/:id/compactions` queues an actual JCode manual compression. The Web renders persisted metrics for trigger, before/after token estimate, saved tokens, summary size, duration and kept messages. Failed or interrupted runs remain failed with partial output and are never replayed automatically. The latest real acceptance fixture compressed 17,491 estimated tokens to 3,292 while retaining all 61 original PostgreSQL messages and recovered its facts after an API restart. Compression failure, invalid metrics, isolation, idempotency and metric recovery also have deterministic integration coverage.

Authorization checks tenant and case owner for every business route. Approval binds an exact proposal digest/version and an idempotency key. Cube create/release uses a durable intent, provider reconciliation and user/case/session metadata; it does not roll back external ERP writes. No endpoint currently writes ERPNext, parses documents, publishes to OpenViking, or exposes general shell access.

Validation: `npm run check` runs Web type checks, API unit/boundary/service tests, Rust builds, isolated PostgreSQL system tests and the Web build. `npm run api:test` includes direct module tests plus service-level tests from `services/api/tests/`, using the public `router(App)` API rather than launching the binary. Every top-level API module has explicit evidence in [TESTING.md](TESTING.md), and architecture tests prevent new modules from silently disappearing from that matrix. Cross-process tests live in repository-level `tests/system/`. `npm run agent:test` runs the bridge Skill boundary tests. The separate upstream `messages_for_provider_applies_manual_compaction_in_native_auto_mode` regression verifies provider history after manual compaction and the session-owned compaction manager.
