# Cube template reuse by JCode

Date: 2026-09-26
Role: scope correction and template interoperability plan
Status: Bounded code-template tool passed local multi-turn, recreation and Web checks; browser-template tool passed one real JCode turn; management UI experiment is not the accepted direction

## User clarification

2026-09-26: The user clarified that other Cube templates should also be useful to JCode. The goal is reusable execution environments and capabilities in Cube, not only a single Forge/JCode application template. Adding a Runtime administration page to Forge, or moving that entire page into Cube WebUI, does not itself satisfy this goal. The previous implementation expanded the management surface before establishing this boundary. Stop extending that experiment; preserve its uncommitted changes and evidence until the useful template/connection work is separated from the unnecessary UI.

The existing flow is: Forge/JCode OCI image -> Cube template -> isolated Bridge/JCode instance -> Forge session binding and authorized turns. The extension is: JCode authorized tool -> backend adapter -> an appropriate Cube template instance -> bounded result and durable artifact references. Cube WebUI remains the template, instance and node management surface. Any Web extension must address a concrete missing step in these flows, not duplicate those views.

## Template roles and compatibility

The inspected upstream checkout is TencentCloud/CubeSandbox commit `f1aaa737fb3862202b1731e0e0d844c28779f930`. Its `web/src/data/templateStore.ts` lists code/Jupyter, browser/Chromium, base/envd and independent application images. Its `docs/zh/guide/templates.md` defines templates as rootfs, runtime configuration and a warmed MicroVM snapshot. A JSON build request is a recipe, not the entire runnable template.

| Template role | JCode integration | Current evidence |
| --- | --- | --- |
| Forge resident application | JCode and Bridge run inside the template instance; Forge binds and configures a business session | Existing resident implementation and local records; previous experiment report has cleanup limitations below |
| Code or browser environment | A controlled tool adapter invokes that environment's supported interface; it need not contain JCode | Code-template `cube_quantity_summary` passed four real JCode/Cube turns, instance recreation and Web persistence checks; browser-template `cube_browser_browse` passed one real turn, PostgreSQL persistence and cleanup below |
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

The official examples need one qualification: the LangChain page shows a short-lived host-side Agent loop that creates one tool sandbox, reuses it during that loop, and kills it at the end; the lifecycle guidance separately supports pausing and resuming the same sandbox. Neither page describes changing the template of an existing running sandbox. Our JCode mapping therefore keeps a PostgreSQL-backed session on one resident compatible sandbox, and performs a template change by creating a new compatible sandbox, revalidating its contract, then restoring durable session state. This is an application design using Cube's documented lifecycle primitives, not a claim that Cube provides an in-place template switch.

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
4. Completed for four synthetic JCode turns, resident recreation, PostgreSQL result/audit checks and desktop/mobile Web persistence. Four concurrent resident sessions also passed on the existing active template; this did not exercise concurrent code-template tools. Multi-template concurrent load, cross-node cleanup and application-session prewarm remain unverified. The business tool timeline still needs safe parameter/result summaries and duration; Cube guest workload metrics and mixed-load alert acceptance remain separate work. The current-host Cube deployment now has a persistent Prometheus/Alertmanager/Grafana baseline; its evidence is recorded in [monitoring-deployment.md](monitoring-deployment.md).

## 2026-09-26 pilot evidence and limits

The local Cube v0.7.2 code template `tpl-9b71f0dec7a54bb1ac07465c` runs the official `sandbox-code` service. A separate resident JCode candidate `tpl-e4b67dd6331e4de5be41f393` was created from image `127.0.0.1:5000/forge-agent-runtime:cube-quantity-20260926` (digest `sha256:9a3f5e7ec13d58eb2230c436fb56e20711610ea80a2d085316bc633a41bb0efb`); its local Cube build was READY. The code image reference uses `latest`, so this pilot is pinned to a Cube template ID, not a recorded immutable code-image digest. The candidate was used only for a canary; the active release remained `e95e0353-cb32-45b3-9f6f-98568161b596` version 1.

`npm run check` passed with local PostgreSQL: 64 API unit tests, three HTTP Router tests, system fixture, Web check/build, MCP/API build. Bridge six tests passed. The new system fixture covers input and token rejection, concurrent same-call deduplication, durable replay after API restart, changed-input conflict, malformed result, lost create acknowledgement, deletion that reports success but leaves an instance, late instance visibility, and sweeper cleanup after restart. These are fixture checks, not separate real Cube failure injections. Failed calls stay failed; cleanup never re-executes the tool. An ambiguous create with no visible instance remains `cleanup_confirmed=false` and is retried by metadata lookup; operator reconciliation is still needed if Cube never reveals it.

Selective commit validation: the 23 pilot files were reconstructed from HEAD in the index and independently checked in `/tmp/erp-cube-quantity-staged-1790409726947` with the clean pinned JCode commit `6819cf87acdc6cc4e2c2c321148eb59c04d82749`. API 61 unit tests and three HTTP tests, Bridge two tests, TypeScript checks, Web build and the full PostgreSQL system fixture passed. API/Bridge `cargo fmt --check` and `git diff --cached --check` passed; all staged files matched that tested tree before this documentation update. The first isolated system run reached and passed the quantity fault tests, then failed because the MCP binary had not been built in the temporary directory; after `cargo build --locked --manifest-path services/mcp-gateway/Cargo.toml -j 6`, the full suite passed. Different test counts in the full worktree include pre-existing experiments that are excluded from this commit.

Real run command: `VERIFY_QUANTITY_RESIDENT_TEMPLATE=tpl-e4b67dd6331e4de5be41f393 VERIFY_QUANTITY_CANDIDATE_ID=c1c888b7-54dc-4da0-8e26-72b2739c6b29 node --env-file=.env --env-file=.runtime/cube.env scripts/verify-cube-quantity-live.mjs`. Report: `/tmp/erp-cube-quantity/1790409061504/report.json`, 2026-09-26 15:51 CST. Synthetic case `7444a63e-14c8-4e55-b6ac-f9deb1e2133f`, message 2213: JCode returned total 50, count 3, minimum 12, maximum 20 with a completed `cube_quantity_summary` event. PostgreSQL operation `5957fe9a-a6a0-4275-8b29-707540851941` bound the code template and separate tool instance `205666e846f045b3b4fffed7a99b78ca`; the resident instance was `613e38cf691541d6b765bbc0a8009ee2`. Result and one completion audit persisted. Both instances were absent from Cube inventory after cleanup, and a forged internal token received 401. The saved run timing reported 507 ms for this tool and 4,538 ms for the run; one local sample is not a latency distribution or capacity result. The API and eight Workers were active after the check.

Two earlier real canary failures informed the fix: Cube rejected integer `erp_message` metadata with HTTP 422, so it is now a string; a candidate adoption check rejected Cube v0.7.2's own `privileged=true` security context, so that asymmetric experiment check was corrected. Reports: `/tmp/erp-cube-quantity/1790406705425/report.json` and `/tmp/erp-cube-quantity/1790406564366/report.json`. The latter correction lives in the uncommitted management experiment, not in the reusable quantity adapter. The verifier's optional candidate path uses that experiment to test a new resident template; its default path creates an ordinary conversation and requires the configured resident template to match.

This is a local code-template canary, not an activated default runtime, a general code execution tool, broad template compatibility, production deployment or 50-user mixed-workload acceptance. Template creation and monitoring continue in Cube WebUI; no new Forge management workflow is accepted. The current official code image tag, guest resource telemetry, cross-node recovery, long-running concurrency, ERP writes, browser templates and parser templates remain separate work.

## 2026-09-26 follow-up acceptance

The bounded code tool remains a canary on resident template `tpl-e4b67dd6331e4de5be41f393` and code template `tpl-9b71f0dec7a54bb1ac07465c`. The active release remained `e95e0353-cb32-45b3-9f6f-98568161b596` version 1; no default runtime was activated. Only synthetic test conversations were used. This round changed verifier scripts and documentation, not the business API, Bridge or service configuration.

Commands (dedicated local credentials are loaded from ignored env files):

```sh
VERIFY_QUANTITY_OUTPUT=/tmp/erp-cube-quantity-acceptance-20260926/quantity-attempt \
VERIFY_QUANTITY_ROUNDS=4 VERIFY_QUANTITY_RECREATE_AFTER=2 \
VERIFY_QUANTITY_RESIDENT_TEMPLATE=tpl-e4b67dd6331e4de5be41f393 \
VERIFY_QUANTITY_CANDIDATE_ID=c1c888b7-54dc-4da0-8e26-72b2739c6b29 \
node --env-file=.env --env-file=.runtime/cube.env scripts/verify-cube-quantity-live.mjs

CUBE_TEMPLATE_ID=tpl-474aeb8eaec04f07a052144b VERIFY_CUBE_COUNT=4 \
VERIFY_CUBE_OUTPUT=/tmp/erp-cube-quantity-acceptance-20260926/concurrent-final \
VERIFY_CUBE_MATCHED_PROMPT='Synthetic acceptance. Call case_read exactly once for the current conversation. After the tool succeeds, reply with exactly CUBE-READ-OK. Do not call any other tools.' \
VERIFY_EXPECTED_ANSWER=CUBE-READ-OK \
node --env-file=.env --env-file=.runtime/cube.env scripts/verify-cube-resident-live.mjs

PLAYWRIGHT_MODULE=/tmp/erp-agent-flow-check/node_modules/playwright/index.mjs \
VERIFY_QUANTITY_REPORT=/tmp/erp-cube-quantity-acceptance-20260926/quantity-attempt/report.json \
VERIFY_QUANTITY_UI_OUTPUT=/tmp/erp-cube-quantity-acceptance-20260926/web \
node --env-file=.env scripts/verify-web-cube-quantity.mjs
```

Quantity report: `/tmp/erp-cube-quantity-acceptance-20260926/quantity-attempt/report.json`, `passed=true`. Messages 2229-2232 in case `7444a63e-14c8-4e55-b6ac-f9deb1e2133f` returned the expected totals 50, 18, 1,000,465 and 0. The third input exercised the 32-item limit and an item at 1,000,000; the fourth exercised a single zero. Each turn made exactly one completed `cube_quantity_summary` call, with matching count/sum/min/max in PostgreSQL and one completion audit. All saved run timings identified backend `cube`. Tool durations were 447, 496, 615 and 530 ms; complete run durations were 3,134, 2,549, 3,630 and 2,704 ms. These four samples do not establish production latency or capacity.

After round two, resident instance `faf9a95ead014c2aaa9ff28aff696658` was deleted; rounds three and four used new instance `5229bea58d7d4fb9915cbe983f9c48d5`. All rounds retained PostgreSQL JCode session `session_3853f2ac4e7b41d08c1eaea3f1344adc` and correctly recalled this attempt's unique marker. The active transcript grew from 29 to 41 entries. Each tool used a separate code-template instance; all four tool instances and both resident instances were absent from Cube inventory after cleanup. A forged token and a valid browser login token both received 401 on the internal tool endpoint with a real session ID, creating no tool operations.

Resident concurrency report: `/tmp/erp-cube-quantity-acceptance-20260926/concurrent-final/report.json`. Four distinct cases, JCode sessions and Cube instances on active template `tpl-474aeb8eaec04f07a052144b` completed two rounds with 8/8 correct replies. The cold round executed one `case_read` per case; the warm round recalled the prior answer without a tool. Observed cold/warm queue P95 was 70/19 ms, first-token P95 3,724/1,791 ms and complete-response P95 3,807/1,872 ms. Peak running counts were four/three; cross-case Bridge credentials received 401 and all four instances were released. This tests the existing resident path, not concurrent quantity-tool environments. The separate 50-session test recorded earlier had 100/100 completed but 93/100 correct arithmetic replies; this small test does not replace that limitation.

Web report: `/tmp/erp-cube-quantity-acceptance-20260926/web/report.json`, `passed=true`. Playwright verified the persisted final reply, localized business label for quantity summary and completed state at 1440, 390 and 320 px, then after reload. Composer and attachment/model/approval/send controls fit the viewport; no horizontal overflow, page errors or HTTP errors were observed. Screenshots are `web/quantity-1440.png`, `web/quantity-390.png`, `web/quantity-320.png` and `web/quantity-1440-reload.png` under the same report directory. The current tool timeline shows label and state only; it does not yet present safe input/result summaries or elapsed time. Those details are durable in PostgreSQL, but backend records are not a complete business-UI acceptance result.

Monitoring sample: the existing `scripts/sample-agent-runtime-resources.mjs` ran with `RESOURCE_OUTPUT=/tmp/erp-cube-quantity-acceptance-20260926/resources.json` at 500 ms intervals during acceptance, then was stopped. There were 557 samples and zero scrape errors. Observed peaks were API RSS 268.6 MiB, runtime RSS 11.4 MiB, four host sandbox metric series, 39 MiB sandbox cgroup charge and 68 PostgreSQL connections; host sandbox memory-limit failures stayed zero. Samples are node-wide and include unrelated work. Guest workload metric series were absent, so no guest working-set or guest failure claim is supported. No retained Prometheus history or alert firing was tested. Cube WebUI, Cubelet, CubeOps, the control target, Forge API and eight Workers were active afterward; API readiness was 200 and Cube WebUI `/health` reported `status=ok` with zero sandboxes after cleanup. This round did not capture the native Cube dashboard with running test instances.

Earlier attempts exposed verifier assumptions rather than new runtime defects: a malformed session ID produced 400 before the browser-token check, result metadata made an exact-object assertion too strict, an old canary marker made recall ambiguous, and the legacy env template differed from the active release. The corrected run used a real session ID, asserted the bounded result fields plus identity metadata, supplied an attempt-specific marker and explicitly matched the active template for the concurrency script. Commands changed only the verifier process environment, not the running release.

`node --check` passed for both changed scripts and `git diff --check` passed. The preceding pilot's full `npm run check` remains the build/system evidence; it was not rerun for these script/document-only edits. Raw reports and screenshots are under `/tmp` and may be removed by local cleanup; this durable summary records the measured results and limits.

Remaining gates: concurrent code-template tools and mixed load, browser/parser adapters, immutable code-image digest capture, safe tool input/result/duration display, guest workload metrics and business-load alert thresholds, full application-session prewarm, cross-node recovery, ERP writes and production deployment. The current-host Cube monitoring baseline is deployed and verified, but it does not establish multi-node capacity, external notification, or production business-load acceptance. Local template interoperability is demonstrated for this one bounded capability; arbitrary Cube templates are not automatically JCode tools.

## 2026-09-26 browser template acceptance

The controlled `cube_browser_browse` adapter uses the official browser template `tpl-5f913dc0f4b3471c92d2fb18` through its Chromium CDP endpoint. The model supplies an HTTPS URL and tool-call ID; backend configuration fixes the template and allowed hosts. The pilot allows only `example.com`, disables general Internet access and requests that host in Cube's `network.allow_out`. It validates the current JCode session/run and persists operation, result, audit and cleanup state in PostgreSQL. This does not expose shell, arbitrary scripts or a general browser controller to the model.

Reproducible command, using ignored local credential files:

```sh
CUBE_API_URL=http://127.0.0.1:3001 VERIFY_API_ORIGIN=http://127.0.0.1:3100 \
node --env-file=.env --env-file=.runtime/cube.env scripts/verify-cube-browser-live.mjs
```

Report `/tmp/erp-cube-browser/1790426894399/report.json` passed at 2026-09-26 20:48 CST. Case `8b875476-eb13-4175-ac6e-6afefc235c63`, message 2246, completed one `cube_browser_browse` call and replied `TITLE=Example Domain` / `HEAD=Example Domain`. Operation `ba71d1c7-9942-426a-b9a9-87e2abd91dfc` persisted the expected title, heading, excerpt and template/instance identity. Browser instance `6cc69f3a87ce47318bd7d9990ebeaa49` was absent from Cube after cleanup; one completion audit was recorded. Forged and browser-login tokens both received 401 without additional operations. Saved timing identified backend `cube`: tool 4,191 ms, complete run 6,642 ms and resident readiness 718 ms. This is one sample, not a percentile or capacity measurement.

Earlier attempts separated three faults: CDP startup briefly returned gateway/connection errors after restore; `Runtime.evaluate` was incorrectly unwrapped twice; a Chromium `ERR_NETWORK_CHANGED` page was initially treated as successful content. The adapter now retries startup briefly, reads the correct value, retries bounded navigations and requires a successful navigation with an HTTPS same-host final URL. The failed-content report `/tmp/erp-cube-browser/1790426633381/report.json` remains evidence of that failure, not a successful browse. UTF-8 excerpt truncation also reserves the ellipsis before choosing a character boundary, with a focused multibyte regression.

`npm run check` passed after the CDP/navigation changes: Web typecheck/build, 65 API unit tests, three HTTP tests, API/MCP builds and the PostgreSQL system suite. That suite's quantity-template fault injections do not establish browser-adapter failure coverage. After the UTF-8-only change, `cargo test --locked --manifest-path services/api/Cargo.toml browser_` passed both browser unit tests, `rustfmt --check --edition 2024 services/api/src/sandboxes/browser_tool.rs` and `git diff --check` passed. The complete API `cargo fmt --check` still reports formatting differences in the existing management experiment (`lib.rs`, lifecycle, provider and management modules); unrelated work was preserved, so the whole-tree formatting gate is not clean. Browser redirects, DNS/egress isolation, cancellation/restart/replay, concurrent browser tools and the business UI trace still require dedicated acceptance. No real customer pages were read.

Local runtime state changed during the earlier management experiment: report `/tmp/erp-runtime-management/1790423360649/report.json` passed and activated release `26184f27-72ef-4c2d-8065-88477a6825bd`, version 2, template `tpl-e30d9fe2d99f47cf963459db`. Its resident image is `127.0.0.1:5000/forge-agent-runtime@sha256:056b864f2e2f89d8219de9b4037d397bfb3974a1d992618dbb35591529ce88c0`. The API was rebuilt/restarted for browser fixes. This supersedes the historical version-1 state above; it is local experiment evidence and does not make that Forge management page the accepted direction. No upstream PR or remote push was performed in this browser follow-up.

Final local service check: the API and eight Worker user units were active, Forge `/ready` returned `ready`, and Cube Web `/health` returned `ok` with zero sandboxes. This historical point-in-time check did not establish monitoring history or alerts; the later current-host monitoring evidence is recorded in [monitoring-deployment.md](monitoring-deployment.md).

## Native Cube Web template selection

2026-09-26: The deployed Cube v0.7.2 Web page `/sandboxes/new` was inspected in a real browser with mock mode off. READY templates were selectable; choosing browser template `tpl-5f913dc0f4b3471c92d2fb18` highlighted it and enabled Create. A screenshot was returned to the user. No instance was created through that page during this read-only inspection.

The matching official `web/src/pages/SandboxNew.tsx` sends the selected `templateID` to `sandboxApi.create`. `SandboxDetail.tsx` and `web/src/api/client.ts` provide lifecycle actions, but no in-place template replacement for an existing generic sandbox. Native AgentHub also accepts a template at creation, but its current create path is for OpenClaw; it does not configure Forge/JCode automatically.

Therefore Cube Web already owns template selection for new instances. The native page does not itself establish Forge's tenant/session identity, Bridge configuration or tool mapping; the Rust API narrow binding flow below now supplies that controlled integration boundary. Tool-template selection instead changes the approved backend capability mapping. These are integration steps, not reasons to rebuild the template manager in Forge. Existing instances keep their current template; moving a conversation to another compatible environment requires a new instance and durable session recovery.

## 2026-09-26 Forge-aware template switch

The first narrow binding flow is now implemented in the Rust API as `POST /v1/cases/{case_id}/sandbox/switch`. Cube remains responsible for template readiness and Sandbox lifecycle. Forge accepts only a template adopted into the local runtime catalog (or the explicitly configured legacy template), checks the existing resident instance and Agent run state, creates a replacement Sandbox with the selected `templateID`, and configures the resident Bridge/JCode against a second runtime lease for the same PostgreSQL session. The replacement remains inactive until Cube reports a provisioned, guest-ready instance. A transaction then promotes the replacement, marks the old binding releasing, revokes the old base/runtime lease, and reconciles the old Cube instance. Message enqueue and Worker provisioning refuse while an inactive replacement exists.

This is orchestration over Cube's documented create, health, configure and delete primitives; it is not an in-place Cube template mutation. The business user sees one switch operation and keeps the same durable conversation/session. A failed replacement is cleaned up and leaves the previously active Sandbox and session lease in place. Cube Web remains the place to build, inspect and monitor templates and generic instances; this API is the controlled Forge/JCode binding boundary, not a second template manager.

The PostgreSQL migration `20260926140000_cube_template_switch` adds additive runtime leases, an `active` Sandbox binding flag and the session-lease reference. The system fixture now verifies successful switching, session continuity, old-lease revocation, post-switch messaging and failed replacement rollback. `npm run build && node --env-file=.env tests/system/test-rust-api.mjs` passed on 2026-09-26 with the Cube fixture. Real Cube Web selection and real resident template acceptance remain separate evidence; this fixture does not claim production deployment or native hot swap.

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

Cube deployment Prometheus retention and Alertmanager delivery are now independently deployed and verified in [monitoring-deployment.md](monitoring-deployment.md). Multi-node capacity routing, arbitrary image registries, configurable commands and egress, guest workload metrics, real new-image build acceptance, rolling migration of existing instances, external notification, and production deployment remain separate gates. Do not infer long-term business-load acceptance from the current single-host monitoring baseline.
