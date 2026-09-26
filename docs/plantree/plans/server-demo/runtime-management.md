# Cube template reuse by JCode

Date: 2026-09-26
Role: scope correction and template interoperability plan
Status: Bounded code-template tool pilot verified locally; management UI experiment is not the accepted direction

## User clarification

2026-09-26: The user clarified that other Cube templates should also be useful to JCode. The goal is reusable execution environments and capabilities in Cube, not only a single Forge/JCode application template. Adding a Runtime administration page to Forge, or moving that entire page into Cube WebUI, does not itself satisfy this goal. The previous implementation expanded the management surface before establishing this boundary. Stop extending that experiment; preserve its uncommitted changes and evidence until the useful template/connection work is separated from the unnecessary UI.

The existing flow is: Forge/JCode OCI image -> Cube template -> isolated Bridge/JCode instance -> Forge session binding and authorized turns. The extension is: JCode authorized tool -> backend adapter -> an appropriate Cube template instance -> bounded result and durable artifact references. Cube WebUI remains the template, instance and node management surface. Any Web extension must address a concrete missing step in these flows, not duplicate those views.

## Template roles and compatibility

The inspected upstream checkout is TencentCloud/CubeSandbox commit `f1aaa737fb3862202b1731e0e0d844c28779f930`. Its `web/src/data/templateStore.ts` lists code/Jupyter, browser/Chromium, base/envd and independent application images. Its `docs/zh/guide/templates.md` defines templates as rootfs, runtime configuration and a warmed MicroVM snapshot. A JSON build request is a recipe, not the entire runnable template.

| Template role | JCode integration | Current evidence |
| --- | --- | --- |
| Forge resident application | JCode and Bridge run inside the template instance; Forge binds and configures a business session | Existing resident implementation and local records; previous experiment report has cleanup limitations below |
| Code or browser environment | A controlled tool adapter invokes that environment's supported interface; it need not contain JCode | Code-template `cube_quantity_summary` pilot passed a real JCode/Cube turn; browser template remains unintegrated |
| Custom document/utility service | A backend adapter passes scoped inputs to the service, persists outputs outside the sandbox and returns references | Design only; no parser/ingestion acceptance implied |
| Another Agent application | Requires its own service contract or a derived image with our Harness; its tools and permissions are not automatically inherited | Not integrated |

Two integration paths are allowed. A compatible derived template can host JCode plus workload dependencies, subject to executable/ABI, startup and application-readiness checks. Alternatively, the resident JCode can use a separate Cube instance as a tool environment. Prefer the latter for reusing existing code/browser services and for bulk jobs; do not require every template to embed another JCode process. Template choice does not hot-swap the running Agent's environment or migrate its memory.

The adapter mapping must identify the workload, pinned template, readiness contract, supported operation and input/output limits. Backend code resolves this mapping; the model does not receive an arbitrary template ID, image, command or network grant. A template's installed software is not automatically a published tool. Existing authorization, tenant/case scope, approvals and idempotency remain authoritative. Both the Agent sandbox and any tool sandbox remain under Cube lifecycle management; durable sessions and job state stay in PostgreSQL and originals/artifacts stay in RustFS.

## Official guidance review

2026-09-26: Read the live Cube documentation below and compared it with the resident implementation. These are upstream examples and interface guidance, not evidence that additional templates work with Forge today. Features in current documentation still require verification against the deployed Cube version.

- [Template overview](https://cubesandbox.com/zh/guide/templates): a template contains the root filesystem, runtime configuration and a warmed MicroVM snapshot. It supplies a reusable environment; it does not automatically install a Harness or publish Agent tools.
- [LangChain integration](https://cubesandbox.com/zh/guide/integrations/langchain): the example binds a tool to a sandbox created from a selected template and replaces local execution with SDK operations. It reuses that sandbox across the Agent loop, preinstalls dependencies and separates sandbox lifetime from command timeout. This supports reusing a tool environment without installing JCode in it. Its generic Python execution permission is not our business Agent policy. The guide also notes that context-manager cleanup can swallow kill errors; Forge must observe cleanup failures and reconcile them.
- [Custom template images](https://cubesandbox.com/zh/guide/tutorials/bring-your-own-image): retain envd for SDK command/file/terminal operations. A template providing only its own HTTP service can omit envd and probe the application instead. Compatibility is therefore the supported interface and workload dependencies, not merely a template ID. Installing envd does not grant the model permission to use its APIs.
- [Prewarmed template services](https://cubesandbox.com/zh/guide/tutorials/prewarm-template-service): the Pi example keeps an initialized AgentSession resident and snapshots it only after its application readiness probe succeeds. Reusable snapshots must exclude credentials and user data; restored external connections need validation or reconstruction, and concurrency must be explicit.

Our recommendation, based on those patterns, is to keep the current resident JCode template and add capability-specific adapters for other Cube template instances. A compatible derived image remains an option when JCode needs that environment directly. One controlled backend adapter may support multiple templates implementing the same verified interface; template-specific software does not require a separate Harness implementation. Cube WebUI owns template/instance operations, while backend configuration binds an approved capability to a pinned template and protocol. A running JCode process does not inherit another instance's packages, workspace or memory merely because both use Cube.

Current readiness gap: `services/runtime-bridge/src/resident.rs` starts with `runtime: None`; template warmup readiness checks the executable, and `/health/ready` can return 200 with `agent_initialized: false`. The first `/v1/turn` constructs JCode. This is service readiness, not the fully initialized Agent-session prewarm described in the Pi example. Before claiming full prewarm, separate safe reusable initialization from post-restore tenant/session configuration; never snapshot a live customer's PostgreSQL session or credentials. The code-template pilot below proves one bounded operation and observable cleanup; browser, parser and other template protocols remain separate acceptance work.

## Target contract

- Keep the resident Bridge, JCode, controlled MCP client, startup command and application readiness probe in the Forge base image/template. Reuse other templates through a compatible derived image or an explicit tool/service adapter. Cube provides isolation and snapshot lifecycle; it does not supply our JCode Harness automatically.
- Keep model credentials, customer data and session identities out of reusable snapshots. After instance creation, the backend validates the instance, binds its tenant/case/session, configures it, and sends authorized turns. Template READY, instance readiness and an operational Agent turn are distinct checks.
- The existing connection implementation is in `infra/cube/agent-template`, `services/api/src/sandboxes/lifecycle.rs` and `services/runtime-bridge/src/resident.rs`. It uses port 49999, `/health/ready`, `/v1/configure` and `/v1/turn`. A generic instance created in Cube WebUI does not currently perform Forge's session binding/configuration by itself.
- ERPNext and OpenViking connect to JCode through controlled business tools/MCP. The current `services/runtime-bridge/src/business_mcp.rs` publishes only four ERP/knowledge MCP tools; `runtime.rs` builds an explicit registry and disables general Bash by default. Additional template capabilities require both a tool adapter and backend permission checks. Skills and todo belong to the Harness. Parsers and batch ingestion use separate service/worker adapters.
- Validate the native template creation/inspection path, actual image/template identity, authorized instance configuration, a real multi-turn conversation and persisted tool/session events. Correlate Cube template/instance IDs with Forge records. Preserve Cube's existing monitoring views and document missing measurements separately.

## Next work

2026-09-26 implementation: `cube_quantity_summary` is a bounded read-only tool for 1-32 integer quantities in `[0, 1_000_000]`. The API binds the active JCode session, tenant, case, run and tool-call ID, pins an operator-configured code template, creates one no-egress Cube instance, executes backend-owned Python through `/execute`, validates count/sum/min/max, records template and instance IDs in PostgreSQL, and confirms instance deletion. The model cannot provide code, template ID, image, shell command, network grant or file path. This demonstrates an additional-template tool path; it is not an ERP calculation or general code interpreter. Local code template: `tpl-9b71f0dec7a54bb1ac07465c` (Cube v0.7.2, official sandbox-code image). The older `tpl-erp-agent-base-20260921` reported READY but failed to create because its snapshot source was missing.

1. Consolidate the existing resident template definition and native Cube creation/inspection path. Retain useful instance identity checks; separate the unaccepted administration experiment without losing its changes or evidence.
2. Completed for the code-template pilot: bounded operation, protocol, backend adapter, permissions, lifecycle, timeout and result contract. Other template protocols need separate adapters.
3. Completed for the pilot through the controlled tool registry; the tool environment and Agent session use distinct Cube instances. No arbitrary shell or implicit session adoption was added.
4. Completed for one synthetic JCode turn and fixture fault paths below. Multi-template concurrent load, cross-node cleanup and application-session prewarm remain unverified.

## 2026-09-26 pilot evidence and limits

The local Cube v0.7.2 code template `tpl-9b71f0dec7a54bb1ac07465c` runs the official `sandbox-code` service. A separate resident JCode candidate `tpl-e4b67dd6331e4de5be41f393` was created from image `127.0.0.1:5000/forge-agent-runtime:cube-quantity-20260926` (digest `sha256:9a3f5e7ec13d58eb2230c436fb56e20711610ea80a2d085316bc633a41bb0efb`); its local Cube build was READY. The code image reference uses `latest`, so this pilot is pinned to a Cube template ID, not a recorded immutable code-image digest. The candidate was used only for a canary; the active release remained `e95e0353-cb32-45b3-9f6f-98568161b596` version 1.

`npm run check` passed with local PostgreSQL: 64 API unit tests, three HTTP Router tests, system fixture, Web check/build, MCP/API build. Bridge six tests passed. The new system fixture covers input and token rejection, concurrent same-call deduplication, durable replay after API restart, changed-input conflict, malformed result, lost create acknowledgement, deletion that reports success but leaves an instance, late instance visibility, and sweeper cleanup after restart. These are fixture checks, not separate real Cube failure injections. Failed calls stay failed; cleanup never re-executes the tool. An ambiguous create with no visible instance remains `cleanup_confirmed=false` and is retried by metadata lookup; operator reconciliation is still needed if Cube never reveals it.

Selective commit validation: the 23 pilot files were reconstructed from HEAD in the index and independently checked in `/tmp/erp-cube-quantity-staged-1790409726947` with the clean pinned JCode commit `6819cf87acdc6cc4e2c2c321148eb59c04d82749`. API 61 unit tests and three HTTP tests, Bridge two tests, TypeScript checks, Web build and the full PostgreSQL system fixture passed. API/Bridge `cargo fmt --check` and `git diff --cached --check` passed; all staged files matched that tested tree before this documentation update. The first isolated system run reached and passed the quantity fault tests, then failed because the MCP binary had not been built in the temporary directory; after `cargo build --locked --manifest-path services/mcp-gateway/Cargo.toml -j 6`, the full suite passed. Different test counts in the full worktree include pre-existing experiments that are excluded from this commit.

Real run command: `VERIFY_QUANTITY_RESIDENT_TEMPLATE=tpl-e4b67dd6331e4de5be41f393 VERIFY_QUANTITY_CANDIDATE_ID=c1c888b7-54dc-4da0-8e26-72b2739c6b29 node --env-file=.env --env-file=.runtime/cube.env scripts/verify-cube-quantity-live.mjs`. Report: `/tmp/erp-cube-quantity/1790409061504/report.json`, 2026-09-26 15:51 CST. Synthetic case `7444a63e-14c8-4e55-b6ac-f9deb1e2133f`, message 2213: JCode returned total 50, count 3, minimum 12, maximum 20 with a completed `cube_quantity_summary` event. PostgreSQL operation `5957fe9a-a6a0-4275-8b29-707540851941` bound the code template and separate tool instance `205666e846f045b3b4fffed7a99b78ca`; the resident instance was `613e38cf691541d6b765bbc0a8009ee2`. Result and one completion audit persisted. Both instances were absent from Cube inventory after cleanup, and a forged internal token received 401. The saved run timing reported 507 ms for this tool and 4,538 ms for the run; one local sample is not a latency distribution or capacity result. The API and eight Workers were active after the check.

Two earlier real canary failures informed the fix: Cube rejected integer `erp_message` metadata with HTTP 422, so it is now a string; a candidate adoption check rejected Cube v0.7.2's own `privileged=true` security context, so that asymmetric experiment check was corrected. Reports: `/tmp/erp-cube-quantity/1790406705425/report.json` and `/tmp/erp-cube-quantity/1790406564366/report.json`. The latter correction lives in the uncommitted management experiment, not in the reusable quantity adapter. The verifier's optional candidate path uses that experiment to test a new resident template; its default path creates an ordinary conversation and requires the configured resident template to match.

This is a local code-template canary, not an activated default runtime, a general code execution tool, broad template compatibility, production deployment or 50-user mixed-workload acceptance. Template creation and monitoring continue in Cube WebUI; no new Forge management workflow is accepted. The current official code image tag, guest resource telemetry, cross-node recovery, long-running concurrency, ERP writes, browser templates and parser templates remain separate work.

## Previous experiment

The following describes the uncommitted management experiment, not the newly clarified product boundary. Its local verification report is `/tmp/erp-runtime-management/1790402197538/report.json`: a new template from the existing approved image and Cube-backed turns were observed. The report's instance cleanup failed, and subsequent fixes were not rerun before the user clarified scope. It is not evidence of native Cube WebUI integration or complete acceptance.

### Experiment boundary

Forge's runtime page is an operator surface backed by the Rust API. Cube WebUI remains a separate infrastructure diagnostic console. `runtime_admin` is a platform-wide role granted explicitly by an operator; registration and the demo seed do not grant it. Browser requests never carry Cube credentials. Runtime administration is not an Agent tool.

A Cube template is a built OCI-derived root filesystem, configuration and warm snapshot. Forge stores a restricted JSON build declaration, Cube's generated template ID and job, the canary evidence, and append-only activation history in PostgreSQL. The request is not Docker Compose and cannot contain commands, environment secrets, arbitrary ports, network grants or tool permissions. The Forge adapter fixes those values. The image reference must be a digest-pinned Forge runtime image from the configured registry prefix.

Cube may host other template types (parsers, ingestion jobs, utilities) and the inventory can show them. Interactive JCode cases may only use templates implementing the Forge resident contract: resident start command, probe on port 49999, approved image origin, scoped runtime configuration, and a successful Cube-resident canary turn. Other workloads need their own backend adapter and lifecycle; a template ID alone cannot attach an arbitrary image to JCode.

### Experiment flow

1. Inventory shows sanitized Cube template summaries, healthy nodes, Forge-managed instances and a live metrics sample. The metrics panel labels a failed scrape as unavailable and does not fabricate a history chart.
2. An operator saves a candidate with a unique request ID, display name, pinned image, CPU and memory. Forge displays the exact restricted Cube build request. Build is at most once; an uncertain Cube acknowledgement is recorded for manual reconciliation, never silently retried.
3. A READY candidate can open a dedicated synthetic conversation bound to that exact template. A completed Cube-resident turn with a successful `case_read` tool event, a persisted sandbox binding and a matching template is the activation gate. The user can inspect the test conversation before activation.
4. Activation compares the runtime configuration version, rechecks READY replicas on healthy nodes, and writes an append-only release record. Only newly created sandboxes select that release. The selected template/release/candidate and last Cube instance ID are preserved on each sandbox row. Previous releases remain selectable for rollback after readiness recheck.

The configured `CUBE_TEMPLATE_ID` remains the explicit legacy fallback until a release is activated. Existing sandboxes and conversations retain their instance and template binding. Release changes do not change ERP approval rules, model tool publication or sandbox egress policy.

### Experiment gates

- API: unauthorized users receive 403 for every runtime route; cross-tenant canary cases are inaccessible; build request fields and image source are bounded; request replay does not repeat a Cube build; activation needs exact canary evidence and optimistic version; sandbox creation pins selection before external effects.
- Browser: desktop and 390/320 px can browse inventory, preview a candidate, see build/test/release states and recover from API/metrics errors without horizontal overflow.
- Local: run `npm run check` with PostgreSQL, inspect a real Cube inventory/metrics read, exercise one synthetic canary and confirm PostgreSQL binding and release selection. A fixture-only build is recorded as fixture validation, not a real Cube build.

### Deferred

Prometheus historical retention/alerts, multi-node capacity routing, arbitrary image registries, configurable commands and egress, guest workload metrics, real new-image build acceptance, rolling migration of existing instances, and production deployment remain separate gates. Do not call the current single-node sample long-term monitoring.
