# Runtime Bridge

Implemented Rust entry point around the pinned JCode `Agent`. The business API
worker launches it with a private stdin/stdout pipe and a dedicated environment.
JCode owns the model/tool loop. The bridge registers `case_read`, JCode's `todo`,
and read-only `skill_find` / `skill_manage` wrappers; it never initializes the
coding/shell/swarm registry.

The current business-read mode also registers two native MCP servers and four
ERP/OpenViking domain tools. ERPNext list/read is verified; OpenViking returns an
explicit unconfigured error until its service and embedding are supplied.
Approval modes ask/auto/full gate the scoped `memory_save`, `case_set_context`
and `proposal_create` actions through Rust API decisions and resumable native
tool calls. The current mode, operation allowlist, read allowlist and business
approver status are injected into each JCode system prompt and are also returned
by `case_read`; the model must report that context instead of guessing. ERP
business approval remains separate and always requires a human approver. See
[verification](../../docs/plantree/plans/server-demo/stream-recovery-fix.md).

## Run

Build from the repository root with `npm run agent:build`. Put a dedicated
`DEEPSEEK_API_KEY` in gitignored `.runtime/deepseek.env` (directory mode 0700,
file mode 0600), then use `npm run dev:api`. Optional settings:

| Setting | Default |
| --- | --- |
| `DEEPSEEK_BASE_URL` | `https://api.deepseek.com` (only accepted origin) |
| `DEEPSEEK_MODEL` | `deepseek-flash`; also accepts `deepseek-v4-pro` |
| `AGENT_RUNTIME_BINARY` | `services/runtime-bridge/target/debug/erp-agent-runtime` |
| `AGENT_STORAGE_ORIGIN` | `http://127.0.0.1:${API_PORT}` |
| `AGENT_COMPACTION_SOFT_RATIO` | `0.80`; background compaction trigger |
| `AGENT_COMPACTION_CRITICAL_RATIO` | `0.90`; synchronous emergency trigger |
| `AGENT_TURN_TIMEOUT_SECONDS` | `0` (no total turn deadline); optional 1–3600 |

The adapter uses JCode's OpenAI-compatible provider with HTTPS transport, the
DeepSeek base URL and its `/models` catalog. Requests use native `/responses`,
including stateless history and function outputs. No Chat Completions fallback
is implemented. Native OpenAI compaction is disabled for this endpoint.

The session budget is the selected model's context window, resolved by JCode
from the model catalog or its model mapping. DeepSeek's API-key `/models` list
does not include window sizes, so the supported `deepseek-flash` and
`deepseek-v4-pro` aliases have a published 1,048,576-token mapping. A catalog
value takes precedence if one is supplied. The default soft and critical
thresholds are 80% and 90% of that window, respectively; both ratios are
configurable, with `0 < soft < critical < 1`. Provider-reported input usage is
preferred when available; otherwise JCode uses its estimate. Compaction
replaces only the provider-facing active prefix with a summary. PostgreSQL
retains the original messages and checkpoint for audit and restart recovery.

Each turn gets an ephemeral HOME/work directory, no developer credentials,
telemetry opt-out, a case-bound storage token and a backend-read case snapshot.
The full business prompt is `prompts/business-agent.md`. `case_read` reads that
turn's authorized snapshot, not ERPNext. The model cannot change its case scope.
Business approval remains a separate API operation.

`skills/business/manifest.json` and four self-contained SKILL.md files compile
into an immutable catalog. Build validation rejects duplicate IDs, symlinks and
oversized content. Runtime registration reuses JCode's Skill parser in a private
directory, prevents first-run imports, and checks that the Agent sees the same
catalog as the tools. No developer skill directory is imported. `skill_find`
matches Chinese/English keywords; `skill_manage` permits list/load/read only.
The published catalog declares the ERP/knowledge MCP tools as dependencies;
discovery reports availability from the active tool registry. Registration does
not prove that an external service is configured. Skill metadata never grants
new tool permissions.

The API archives each catalog by SHA-256 in PostgreSQL and records each run's
catalog hash and loaded Skill versions. Full loaded content is retained as a
JCode tool result. Each turn uses the binary's fixed catalog; a later deployment
may change a subsequent turn's catalog, with both versions retained in history.
An interrupted run remains failed rather than automatically replaying it.

PostgreSQL is the sole durable session store. The existing upstream storage patch
handles JCode messages, tool results and todo documents through scoped internal
routes with CAS/fencing. Temporary PID/identity files are removed when the child
exits; no JSON/JSONL session copies are retained. The upstream TypeScript SDK/server
still assumes file-backed history in places, so this adapter calls the Rust Agent
directly instead of claiming that the SDK/socket route has been integrated.

The bridge forwards answer text, provider reasoning, tool status and terminal
events. The Web separates reasoning into an expandable ordered timeline and
smooths answer chunks with Astryx's streaming hook. Provider diagnostics remain
private and redact the model key and storage token. The API persists partial
replies and ordered steps; the Web polls them at 600 ms after each read. The
queue permits one active turn per case, deduplicates message IDs, and uses a
PostgreSQL advisory lock across workers. Orphaned running jobs are marked failed
and their storage token revoked; they are never automatically replayed. Queued
jobs survive restart and recheck the sender's current authorization.

## Verified Scope

See [the initial integration record](../../docs/plantree/plans/server-demo/deepseek-integration.md)
and [Skill/protocol verification](../../docs/plantree/plans/server-demo/skill-parser-integration.md)
for real model/tool calls, Skill version and todo persistence, process-restart
recovery, UI screenshots and fault/concurrency tests. Current placement is an external host process, not a
resident Cube runtime. It is a first integration, not production isolation or a
capacity benchmark: one worker turn at a time, no default wall-clock deadline,
4,096 output-token budget, and bounded text/tool events. A deadline can be set
explicitly with `AGENT_TURN_TIMEOUT_SECONDS`; stopping or replacing a process
first gives JCode time to persist its partial answer and close the turn.
Visible text is checkpointed in PostgreSQL before it is forwarded to the Web,
so a process crash can restore that answer once, on the next turn.
Long-context compaction, ERP MCP reads and real cancellation UI are now verified;
see [latest integration](../../docs/plantree/plans/server-demo/mcp-composer-integration.md).
Cube placement, real knowledge retrieval, warm pools and
mixed-load throughput remain separate acceptance work.
