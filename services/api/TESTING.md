# API Test Matrix

The API uses direct unit tests for deterministic rules and real PostgreSQL integration tests for repositories, locks, transactions and route composition. Controlled HTTP, S3, Cube, ERP, OpenViking and runtime fixtures test adapters without touching production accounts or the normal development bucket.

`npm run api:test` runs Rust unit, architecture and service integration tests, including `tests/http_service.rs` against the public library router. `npm run check` also builds the API/MCP/Web and runs the cross-process harnesses in the repository-level `tests/system/` directory against a randomly named PostgreSQL database.

| Module | Direct tests | Integration evidence |
| --- | --- | --- |
| `agent_sessions` | Session ID generation, delta metadata/vector limits, append/replace heads, document kind/schema/revision/size | Registration/binding, claim fencing, old-token rejection, CAS, receipts, changed replay rejection, documents and restart recovery |
| `auth` | Email normalization, bearer grammar, Argon2id, legacy scrypt, malformed hashes, token generation, exact roles | Registration conflicts/concurrency, wrong password/tenant, legacy upgrade, owner isolation, logout revocation |
| `cases` | Context/proposal/approval validation, transitions, normalization, digest compatibility, pagination | Creation, optional context, proposals, approvals, locks, idempotency, evidence/role isolation and audit rollback |
| `conversation` | Operation-bound replay, cancellation transition, title normalization | Send/history/settings, active-run conflicts, cancellation, pagination, compaction retry/recovery and audit rollback |
| `integrations` | Settings/tool schema, ERP scope/limits/order input, OpenViking URI/query limits, response status/JSON/size and field allowlists | Scoped runtime token, active run, explicit grants, controlled ERP/OpenViking requests, isolation and all approval modes |
| `objects` | Upload path/type/encoding/size and retry fingerprint | Controlled S3 upload retry/failure recovery/hash rejection, downloads, listing, ownership, memory limits and transactional Agent memory |
| `permissions` | Human-business-approval separation, exact decision replay, call/message binding, expiry and active-run rules | Pause/restart/resume, exact digest, duplicate decisions, full/auto/ask, reviewer failure, expiry, rejection and cancellation |
| `platform` | Hashing, strict parsing, text/UUID/session guards and public error contracts | Migration id compatibility, checksum drift rejection, readiness and independent test database lifecycle |
| `sandboxes` | Provider metadata scope, remote ID, ready replica/node matching and public projection | Lost-create reconciliation, duplicate prevention, user metadata, uncertain/asynchronous deletion and owner isolation |
| `worker` | Catalog digest/schema/source/unique IDs, reasoning/text/tool/compaction stream events and output limits | Two-process serialization, runtime failures, unpublished tools, skill mismatch, orphan fencing, shutdown checkpoint, long turn and authorization recheck |

Architecture tests additionally enforce thin HTTP handlers, declaration-only `mod.rs` files, SQL-free layered applications and transaction-free repositories. The matrix asserts that every top-level API module remains registered here; it does not replace behavioral assertions.

Placement follows service ownership:

- `src/**/tests.rs` or inline `#[cfg(test)]`: private module and domain rules.
- `services/api/tests/*.rs`: service-level Rust integration tests through public library APIs.
- `services/api/tests/support/`: shared integration-test setup; it is a submodule rather than an independent Cargo test target.
- repository-level `tests/system/`: cross-process API, Runtime, MCP, Cube and controlled external-service tests.
