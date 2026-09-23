# Mindloop Local Cleanup

Status: Complete, 2026-09-21 23:45 CST.

## Authorization and Boundary

The user explicitly authorized removing all Mindloop-related local resources.
This cleanup removes confirmed Mindloop resources while retaining CubeSandbox,
ERP Agent, ERPNext, CarbonCheck, PetMatch, other applications, developer chat
history, and shared platform infrastructure.

Cube storage was moved by the parent task to
`/var/lib/cubesandbox/cubelet-xfs.img` and mounted at `/data/cubelet` before this
cleanup. Neither that image nor Cube configuration is in the deletion scope.
The mixed `/home/axia/new-chat` repository is retained.

## Plan

- [x] Identify running services, Kubernetes ownership, Docker mounts, and source ownership.
- [x] Stop and remove Mindloop workloads and private browser services.
- [x] Remove confirmed private images, volumes, runtime stores, and source/build directory.
- [x] Verify retained workloads, remaining resources, and reclaimed disk space.

## Initial Evidence

- Root filesystem used: 862,632,980,480 bytes; available: 92,617,641,984 bytes.
- `sandbox/README.md` identifies `/home/axia/new-chat/sandbox` as MindLoop Sandbox.
  Allocated size: 115,860,836,352 bytes.
- `/var/lib/mindloop` allocated size after Cube migration: 13,196,521,472 bytes.
- Kubernetes has the four standard namespaces and `mindloop-local-k8s` only.
  Mindloop owns 164 pods in its namespace, 7 named maintenance pods in
  `kube-system`, 5 `mindloop.dev` CRDs, 6 cluster RBAC objects, and one Forgejo PVC.
- Mindloop custom resource counts: 1 pool, 20 sandboxes, 1 node health,
  362 prepared allocation requests, and 3,004 workspace prepared demands.
- Docker has 106 Mindloop image tags and `mindloop-local-registry` with a private
  anonymous registry volume.
- Twelve stopped `local-dev` Docker containers belong to
  `/tmp/e2b-infra/packages/local-dev`; the local Mindloop comparison helper
  `tasks/e2b-selfhost-preflight.sh` names this source and its
  `.mindloop-localdev-ports.env` manifest. These are the retired Mindloop E2B
  comparison stack, not the current Cube stack.
- User services `mindloop-browser-chrome` and `mindloop-browser-cdp-proxy` use
  private Mindloop profile and proxy paths. The system
  `mindloop-firecracker-cow-store` service references Mindloop source and storage.

Resource names and mounts were inventoried without environment values or secrets.
Detailed transient inventory: `/tmp/mindloop-cleanup-inventory.json`.

## Removed

- User services, unit files, private Chrome profile, and proxy program:
  `mindloop-browser-chrome`, `mindloop-browser-cdp-proxy`.
- Inactive system unit `mindloop-firecracker-cow-store.service`.
- `/home/axia/new-chat/sandbox`, `/var/lib/mindloop`, and the matching `sandbox`
  copy in `output/worktrees/carboncheck-seo-20260907`.
- Dedicated `infographic/mindloop-sandbox` directories in the main checkout and
  that worktree. Mixed CarbonCheck documents were retained.
- Kubernetes namespace `mindloop-local-k8s`, its workloads, private Forgejo PVC
  and backing local-path PV directory; seven named Mindloop maintenance pods in
  `kube-system`; all five `mindloop.dev` CRDs; six Mindloop cluster RBAC objects.
  The 20 sandbox finalizers were cleared only after the operator and node agent
  were stopped, no Mindloop runtime processes remained, and private runtime
  storage was removed. The namespace and CRDs then disappeared normally.
- Three Mindloop image references in k3s containerd.
- Mindloop node label and the corresponding `--node-label` argument in the k3s
  unit. The shared k3s service was reloaded, not restarted or uninstalled.
- Docker registry container `mindloop-local-registry`, 106 Mindloop image tags,
  and these retired E2B comparison containers: `local-dev-vector-1`,
  `local-dev-tempo-1`, `local-dev-postgres-1`, `memcached`, `local-dev-redis-1`,
  `local-dev-socks5-1`, `local-dev-tempo-init-1`, `local-dev-mimir-1`,
  `local-dev-loki-1`, `local-dev-clickhouse-1`, `local-dev-grafana-1`,
  `local-dev-otel-collector-1`.
- Six private Docker volumes (three named `local-dev` volumes and three
  anonymous volumes identified from those containers), plus `local-dev_default`.

## Verification

At 2026-09-21 23:44:57 CST:

- No deleted Mindloop paths, Docker containers/tags, namespace, CRDs, RBAC
  objects, or private PV remained. No Mindloop runtime processes were running.
- All 30 non-Mindloop Docker containers from the initial inventory remained.
  ERP Agent PostgreSQL, ERPNext, CarbonCheck, OpenDesign, and AI OPCC running
  containers remained running. Previously stopped PetMatch and other containers
  were retained in their previous state.
- Shared k3s CoreDNS, local-path provisioner, and metrics-server were Running
  and Ready. The four standard namespaces remain.
- `/data/cubelet` still mounts `/dev/loop0` as XFS, and
  `/var/lib/cubesandbox/cubelet-xfs.img` exists. Cube services and configuration
  were not changed by this cleanup. Cube runtime repair/acceptance belongs to
  the parent integration task; this preservation check does not prove VM boot.
- Root used bytes decreased from 862,632,980,480 to 698,694,049,792.
  Net observed recovery: **163,938,930,688 bytes (152.68 GiB)**.
  Available space increased to 256,556,572,672 bytes; usage decreased from
  **91% to 74%**. These are live filesystem measurements, so concurrent activity
  can cause small changes after the sample.

## Intentionally Retained

- Mixed `/home/axia/new-chat` repository, its Git history, CarbonCheck source
  (including `CarbonCheck/carbon_check/mindloop`), and shared CarbonCheck
  documentation/presentations. Removing that application module would be a
  separate CarbonCheck code change, not removal of the retired runtime.
- Developer chat history and `/home/axia/.mindloop/memory`.
- Shared k3s platform, generic Docker base images and unclassified build cache,
  and unrelated/anonymous Docker volumes without confirmed Mindloop ownership.
  No global Docker/containerd prune was run.
- Cube, ERP Agent, ERPNext, PetMatch, CarbonCheck, OpenDesign, and AI OPCC state.

Machine-readable deletion names and verification evidence:
`mindloop-cleanup-evidence.json` next to this document. Transient full inventory
and validation are also available under `/tmp/mindloop-cleanup-*.json`.
