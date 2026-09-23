# Deployment

`compose.yaml` starts this project's PostgreSQL development database on loopback port
55432. Its persistent volume is scoped by the `erp-agent` Compose project. No other
service is deployed by this configuration. The development database account is a
bootstrap superuser; production requires separate migration/runtime roles and backups.

Candidate inside each case sandbox: Runtime Bridge, JCode, stdio MCP entrypoint, case
workspace. Placement is subject to the comparison in
[`cube-runtime-placement.md`](../docs/architecture/cube-runtime-placement.md).

Outside sandboxes: business API/database, Cube control plane, ERP adapter/ERPNext, knowledge authorization/OpenViking, original/artifact storage, durable queue/workers, shared GPU model services.

Before installing Cube, inventory target Linux/KVM or PVM support, filesystem/XFS reflink, disk, memory, ports, and network policy. GPU services can live on a separate node. Do not install a replacement host kernel or reboot a shared machine as incidental setup.

OpenViking and Cube remain independent upstream services. Track their image/template
versions and local config here when compatibility is verified; do not pull mutable
upstream branches at runtime. This project has not installed or modified existing Cube
or ERPNext stacks on the development machine.
