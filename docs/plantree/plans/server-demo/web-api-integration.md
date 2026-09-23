# Web API Integration

Status: Complete for business Web, conversation persistence and local Cube lifecycle (2026-09-22). Model execution is deferred by the user.

## Current Scope

- Use the existing authenticated business API for cases, proposals, approvals and audit.
- Persist case conversation inputs in PostgreSQL before any future runtime dispatch. Assign a case-bound JCode storage session on the server; never expose storage credentials to the Web.
- Add idempotent message submission, scoped history pagination, explicit save errors and reload recovery. An offline runtime must not produce simulated assistant or tool responses in an authenticated case.
- Verify login, case creation, message save/reload, proposal review and approval through the actual browser/API/database chain using synthetic local data.
- Keep infrastructure state out of the business UI: no sandbox-ready or Agent-disconnected banners, and no sandbox lifecycle rows in business activity. Preserve those events in the backend audit. Tool rows name ERPNext/OpenViking operations, without an MCP suffix.

## Runtime Boundary

The repository contains a PostgreSQL storage API and a JCode remote persistence patch that need acceptance testing. There is no running runtime bridge or project-specific model configuration. Saved inputs do not automatically start or queue model execution. Preserve the JCode loop decision; do not substitute a frontend timer or a custom model loop.

JCode provider configuration is deferred by the user. Cube lifecycle integration is now authorized: fixed backend template, case-scoped create/status/release, PostgreSQL intent before external creation, reconciliation after uncertain responses, and durable expiry cleanup. ERPNext tools and OpenViking retrieval remain separate integration gates.

Cube recovery found a missing XFS mount, stale node/proxy IP, an old-node template replica and a cached proxy route. The existing disk was moved to `/var/lib/cubesandbox/cubelet-xfs.img` and mounted with a persistent systemd unit. A dedicated base template now creates successfully; guest health through the proxy returned 200 and explicit deletion returned 204. This template does not contain JCode/Bridge. The installed Cube API's timeout did not release the probe within seven minutes, so the application must track its own expiry and retry cleanup after restart; automatic expiry requires the API process to be running.

## Acceptance

- PostgreSQL tests cover duplicate requests, changed retries, tenant/owner/role isolation, pagination, restart recovery and atomic audit writes.
- Web sends to the local API, restores messages after a reload, and records an exact-version approval with the resulting audit entry.
- Live mode never shows demo tools or fabricated replies. Mobile layout and grayscale styling remain intact.

## Evidence

- `npm run check`: strict types, all 16 isolated PostgreSQL tests, and API build passed. Tests include session storage, conversation retries/pagination/atomic audit, Cube concurrent creation, uncertain create/delete responses, tenant/owner boundaries, stale releases and expiry cleanup.
- `npm run preview:build`: Web typecheck, production build and static preview generation passed.
- Headless Chrome through Playwright exercised real login, new case, save, reload, synthetic proposal creation through the API, approval through the Web, and a second reload. PostgreSQL contains one message, one conversation and one approval bound to the exact proposal ID/digest. No ERP write occurred.
- Cube create, idempotent replay, proxy guest health, restored case binding and verified release passed through the authenticated business API. One warm sample measured 84 ms create and 72 ms release including database work; these are not performance benchmarks or model latency measurements.
- A separate API process stop/start preserved the exact message IDs, JCode storage session ID and active Cube instance. Guest health passed after restart; the instance was then released successfully.
- Desktop 1440x1000 and mobile 390x844 / 320x844 screenshots show the composer inside the viewport, no horizontal overflow, and no infrastructure banners. No browser page errors were observed.
- Fixed empty requests incorrectly carrying `Content-Type: application/json`, which Fastify rejected with 400; the shared Web API client now sends that header only with a body.
- Durable reports: [browser/API/database evidence](web-api-integration-evidence.json), [process restart evidence](web-api-restart-evidence.json). Screenshots: `/tmp/erp-live-integration/{desktop-saved,desktop-proposal,desktop-approved,mobile-390-proposal,mobile-320-proposal}.png`.
- Final UI check after filtering infrastructure events: no connection banners or sandbox rows, business events remain visible, logout returned 204 and removed the local token. No page errors. [Report](web-ui-final-evidence.json); latest screenshots: `/tmp/erp-live-integration/final-desktop.png` and `/tmp/erp-live-integration/final-mobile.png`. The final Cube inventory contained zero active instances.
- Local recovery details and limitations: [Cube operations](../../../../infra/cube/README.md). Separate authorized removal: [Mindloop cleanup](mindloop-cleanup.md).

Local preview: `http://127.0.0.1:4173`; API: `http://127.0.0.1:3100`. This is local integration acceptance, not production deployment. JCode model/Bridge execution, real ERPNext tools and OpenViking retrieval remain unverified and are not represented as live chat responses.
