# Project Instructions

## Resume

- Read `README.md`, `docs/plantree/README.md`, and the active plan before implementation.
- The Rust business API, PostgreSQL persistence, RustFS originals, local Cube lifecycle and JCode/DeepSeek Responses loop have passed local integration checks. ERPNext tools, knowledge retrieval and a resident Cube Harness remain unintegrated; see the active plan for evidence.
- Keep durable decisions and progress in `docs/plantree/`; do not rely on chat history alone.
- Do not claim deployment, performance, integration, or acceptance without observed evidence.

## Accepted Direction

- Use JCode as the Rust Agent Harness. Build the business Web UI ourselves.
- Include CubeSandbox in the first demo. Prefer testing resident JCode/SDK bridge inside the sandbox, but compare external Harness placement before finalizing; see `docs/architecture/cube-runtime-placement.md`.
- Keep todo and scoped read tools. Disable general Bash, arbitrary file edits, and unrelated coding/swarm tools exposed to the business model.
- This restriction applies to the product Agent, not to the developer's authorized build and diagnostic shell commands.
- Use ERPNext for the demo ERP, with domain MCP tools and synthetic civilian manufacturing fixtures.
- Use OpenViking for scoped knowledge retrieval and MinerU for document parsing.
- Run bulk ingestion through a durable queue and asynchronous workers outside interactive case sandboxes.
- Keep shared GPU model services outside per-case sandboxes. Reuse warm instances and connections; measure end-to-end performance.

## Boundaries

- Enforce identity, authorization, approval, idempotency, ERP state checks, and version rules in backend code.
- Agent todo is analysis progress, not proof of procurement, production, or delivery completion.
- Bind approval to an exact proposal version and action list; revalidate ERP state before writes.
- Keep business records, original documents, audit events, and published artifacts durable outside sandboxes.
- JCode sessions, messages, tool results, todo and compaction/recovery state must use PostgreSQL as the sole durable store. Do not retain JSON/JSONL session files with periodic DB syncing. Real messages, tools, todo, long-context compaction, original transcript retention and host-process recovery have passed; Cube-resident recovery remains unverified. See `docs/architecture/session-storage.md`.
- Sandbox snapshots do not roll back external ERP writes. Verify writes before retrying after disconnects.
- Preserve customer, project, part, revision, effectivity, permissions, and source locations in knowledge retrieval.
- Use dedicated runtime state and credentials. Never inherit developer logins or commit secrets and real customer files.

## Implementation

- Follow the module ownership in the README. Avoid splitting more services without a demonstrated need.
- PostgreSQL is confirmed (not PostgREST). The self-built API is Rust/Axum, organized by business module. React/TypeScript Web and a Rust JCode Bridge are implemented; the TypeScript SDK/socket route remains unintegrated.
- JCode lives in `external/jcode` as a pinned submodule; preserve its upstream and keep updates explicit. OpenViking and Cube are independent services, with deployment definitions owned here.
- Inspect pinned upstream versions before relying on SDK, MCP, prompt injection, or persistence behavior.
- Keep dependency locks and schemas versioned once code is introduced.
- Test workflow correctness, restart recovery, duplicate writes, cross-case isolation, and mixed online/batch load according to risk.
- Record measured cold/warm startup, TTFT, tool latency, throughput, resource use, and correctness together.

## Current Scope

The foundation covers auth, cases, proposals, approvals, audit, saved conversations, RustFS originals, personal memory, Cube lifecycle and a real JCode/DeepSeek Responses loop. New inputs queue model work when dedicated configuration is present; historical saved inputs are not replayed. The model exposes only case_read, todo, skill_find and read-only skill_manage. Approval does not execute ERP changes. Keep sandbox readiness and Agent connection diagnostics out of the business UI. Cube-resident JCode, production deployment, ingestion and ERP/knowledge acceptance remain separate work. Run `npm run check` against the local PostgreSQL container after business API changes.
