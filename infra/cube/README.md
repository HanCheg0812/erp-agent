# Local Cube Operations

Verified on 2026-09-22 against the existing installation in
`/usr/local/services/cubetoolbox`. This recovers a local installation; it is not a fresh
deployment recipe or acceptance of the JCode runtime.

## Observed Versions and Endpoints

- Cube API v0.4.0, commit `4004a6ec34a9d045a9789a1fd438d6518eedb3d3`.
- Installed custom Cubelet 0.0.0-dev, commit `15ad2ac8d993d70ccaa8f3137f260450799ea344`.
- API: `http://127.0.0.1:3001`; node inventory: `/cubeapi/v1/nodes` on that API.
- CubeMaster: port 8089; proxy: host ports 80/443; guest health: port 49999.
- Active node IP: `192.168.100.122` at verification time. Recheck before reusing scripts.
- Project template: `tpl-erp-agent-base-20260921`, READY on the healthy current node.
  This is a lightweight code-interpreter base; JCode and Bridge are not installed.

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

Next runtime work is a JCode/Bridge template, dedicated model configuration, scoped tools
and PostgreSQL recovery acceptance. Cold/warm model startup, TTFT, mixed workloads and
Harness placement comparisons have not been measured.
