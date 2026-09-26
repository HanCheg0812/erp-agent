# Local Cube Operations

The local installation in `/usr/local/services/cubetoolbox` was repaired on
2026-09-22 and upgraded to v0.7.2 on 2026-09-25. This is an observed local
configuration, not a fresh deployment recipe or production acceptance.

## Observed Versions and Endpoints

- Web UI: `http://127.0.0.1:12088/`; local deployment and login verification
  are recorded in [the Web UI report](../../docs/plantree/plans/server-demo/cube-webui.md).
- Current Cube CLI/API package: v0.7.2. The previous v0.4.0 API and custom
  Cubelet 0.0.0-dev are historical and no longer the active installation.
- API: `http://127.0.0.1:3001`; node inventory: `/cubeapi/v1/nodes` on that API.
- CubeMaster: port 8089; proxy: host ports 80/443; guest health: port 49999.
- Active node IP: `192.168.100.122` at verification time. Recheck before reusing scripts.
- Current resident template: `tpl-00f062671d894be6808685fa`, READY on the
  local node. The older `tpl-erp-agent-base-20260921` is only a lightweight base.

The local Cube configuration has `CUBE_SANDBOX_MINIO_ENABLED=0`,
`CUBE_ARTIFACT_STORE_BACKEND=fs`, and `CUBE_OPS_STORE_BACKEND=fs`. Cube's
one-click installer defaults to MinIO for S3-compatible volumes, but template
artifacts and CubeOps can use filesystem storage. This setup uses no Cube S3
volumes, so MinIO is not required. Forge originals are stored separately in
RustFS; JCode session state is stored in PostgreSQL. Cube S3 volumes would
require MinIO or another S3-compatible service.
The MinIO systemd unit is masked and no MinIO container is running. Its stale
`failed` state records an upgrade-time attempt to start without a password;
it is not an active dependency failure. Cube API, CubeOps and the resident
template remain healthy with the filesystem backends above.

## Faults and Recovery

1. Cubelet repeatedly failed `FICLONE` with `EOPNOTSUPP`. `/data/cubelet` was on ext4
   because the existing XFS image was not mounted. The retained 200 GB sparse image
   was moved from `/var/lib/mindloop/cubesandbox/cubelet-xfs.img` to
   `/var/lib/cubesandbox/cubelet-xfs.img`, then mounted as XFS with reflink enabled.
   No formatting occurred. `data-cubelet.mount` is enabled, and the network-agent
   drop-in requires that mount; Cubelet already had a mount dependency.
2. The installation still used node IP `192.168.3.5`. Updating the node configuration
   restored a healthy current-node registration. The stale unhealthy record remains.
3. Old template `tpl-ibt20260628090657` only had a replica on the former node.
   Its redo path lacked a build job. `recover-template.mjs` uses its existing creation
   request and the installed CLI to create the project template on the current node.
4. The proxy also retained the old host IP. Updating its configuration passed nginx
   validation, but reload retained a stale Lua route cache and guest requests returned
   504. Direct guest health returned 200. Restarting only `cube-proxy` cleared the cache,
   after which proxy guest health returned 200.

`repair-local-storage.sh` documents the guarded original migration and refuses to run
when storage is already mounted. It never formats a disk. The template recovery script
refuses to replace an existing target and removes its temporary private request file.
These scripts target the recorded local installation, not arbitrary servers. Inspect
state before reuse; do not print upstream template requests or configuration secrets.
Host reboot recovery has not been tested.

## Lifecycle Acceptance and Limits

The authenticated business API has created a case-bound instance, checked proxy guest
health, replayed creation idempotently, recovered the same instance after an API process
restart, and released it with provider absence confirmed. See the durable reports in
[Web/API integration](../../docs/plantree/plans/server-demo/web-api-integration.md).
The probes used synthetic local data and performed no ERP writes or model requests.

The installed provider did not expire a 180-second probe within seven minutes.
The business API therefore stores its own 15-minute lease and runs a cleanup sweep on
startup and every 30 seconds. If that process is stopped, cleanup waits for restart;
this is not a provider-level TTL guarantee. Uncertain create/delete results are retained
and reconciled without blindly creating replacements.

The JCode/Bridge guest contract is in
[agent-template](agent-template/README.md): it builds an Ubuntu 24.04 resident HTTP
service with `/health/live`, `/health/ready` and authenticated `/v1/turn`. In
`AGENT_RUNTIME_MODE=resident`, the first turn initializes an in-process JCode Agent
and subsequent turns reuse it; the default stdin/stdout path remains available for
rollback. The previous v0.4.0 CubeVS prevented guest traffic to the host's own
LAN IP. After upgrading to v0.7.2 and rebuilding the resident template, a real
JCode/DeepSeek first turn and a later turn after API/Worker restart completed
inside the same Cube instance and PostgreSQL session. A second synthetic case
used a separate instance/session. Both completed ERP list/read tools;
OpenViking returned its expected unconfigured-service error. The resident
Bridge now sends `skill_catalog` before every turn, matching the Worker's
per-run validation. See [the resident runtime plan](../../docs/plantree/plans/server-demo/cube-resident-runtime.md)
for IDs, failures, and remaining acceptance work. A synthetic JCode pause/resume
and recreate lifecycle now passes twice on the current template. Multiple
pause/resume cycles on one instance,
simultaneous business load, model TTFT, and placement comparisons remain
unmeasured. This Cube v0.7.2 setup requires the guest DNS resolver IPv4 address
in `CUBE_DNS_RESOLVER` (default `169.254.254.53`) so a restricted sandbox can
still resolve domain allow rules after pause/resume.

Cube v0.7.2 rewrites non-presigned HTTP template artifact URLs to the
CubeMaster host. The current single-node template build staged its ext4
artifact in the node image directory before registration; this is not a
multi-node artifact distribution procedure.
