# CubeSandbox monitoring

This stack is the persistent monitoring deployment for the CubeSandbox control node and its Forge integration. It is intentionally bound to loopback: it is for the operator on this machine, not an Internet-facing monitoring service.

The monitoring control plane belongs beside Cube's control services, not inside each JCode or business template. Templates and sandboxes expose runtime/resource signals; one control-plane Prometheus/Grafana/Alertmanager deployment keeps those signals continuous across template switches and sandbox replacement.

The stack contains Prometheus (30-day retention), Alertmanager, Blackbox Exporter, Node Exporter, PostgreSQL Exporter, and Grafana alongside the CubeSandbox control node. Prometheus scrapes Cubelet's native `/v1/metrics/resource` endpoint and the Cube control-plane health endpoints. Alertmanager sends alerts to the local `erp-agent-alert-sink.service`, which appends a sanitized JSONL record under `.runtime/monitoring/alerts.jsonl`. Its single-node gossip listener is also loopback-only. Grafana provisions the `CubeSandbox Observability` and `Forge ERP Agent` dashboards plus the Prometheus data source on first start.

## Deploy

From the repository root:

```sh
npm run build
npm run monitoring:configure
mkdir -p "$HOME/.config/systemd/user"
systemctl --user link "$PWD/infra/systemd/erp-agent-api.service"
systemctl --user link "$PWD/infra/systemd/erp-agent-worker@.service"
systemctl --user link "$PWD/infra/systemd/erp-agent-alert-sink.service"
systemctl --user link "$PWD/infra/systemd/erp-agent-monitoring-health.service"
systemctl --user link "$PWD/infra/systemd/erp-agent-monitoring-health.timer"
systemctl --user daemon-reload
systemctl --user stop erp-agent-dev-api-pool.service erp-agent-dev-worker8-{1..8}.service
systemctl --user enable --now erp-agent-alert-sink.service erp-agent-api.service erp-agent-monitoring-health.timer
systemctl --user enable --now erp-agent-worker@{1..8}.service
docker compose --env-file .runtime/monitoring.env -f infra/monitoring/compose.yaml up -d
```

`npm run monitoring:configure` records the current host UID/GID so the read-only Node Exporter container can read the user-owned health snapshot without running as root.

The user services are:

| Service | Purpose |
| --- | --- |
| `erp-agent-api.service` | Public API on `127.0.0.1:3100`; private metrics on `127.0.0.1:3101` |
| `erp-agent-worker@1..8.service` | Eight persistent Agent workers |
| `erp-agent-alert-sink.service` | Loopback Alertmanager webhook receiver on `127.0.0.1:29198` |
| `erp-agent-monitoring-health.timer` | 30-second host/Cube/API health snapshot |

The operator endpoints are:

| Endpoint | Purpose |
| --- | --- |
| `http://127.0.0.1:13200` | Grafana; credentials are generated in `.runtime/monitoring.env` |
| `http://127.0.0.1:29190` | Prometheus |
| `http://127.0.0.1:29193` | Alertmanager |
| `http://127.0.0.1:29115` | Blackbox Exporter |

Cube resource metrics are expected to be empty when the Cubelet node has no running sandbox. The `cube-sandbox-availability` rules treat an unavailable endpoint or inactive Cube service as a failure, while an empty 200 response is not itself an alert.

## Verify

```sh
systemctl --user --no-pager --full status erp-agent-api.service erp-agent-alert-sink.service erp-agent-monitoring-health.timer
systemctl --user --no-pager --full status 'erp-agent-worker@*.service'
docker compose --env-file .runtime/monitoring.env -f infra/monitoring/compose.yaml ps
curl -fsS http://127.0.0.1:3100/ready
curl -fsS http://127.0.0.1:3101/metrics | sed -n '1,80p'
curl -fsS http://127.0.0.1:29190/-/ready
curl -fsS http://127.0.0.1:29193/-/ready
curl -fsS http://127.0.0.1:13200/api/health
curl -fsS http://127.0.0.1:29198/health
```

Check target state and rule validity from the containers:

```sh
docker compose --env-file .runtime/monitoring.env -f infra/monitoring/compose.yaml exec -T prometheus promtool check config /etc/prometheus/prometheus.yml
docker compose --env-file .runtime/monitoring.env -f infra/monitoring/compose.yaml exec -T prometheus promtool check rules /etc/prometheus/rules/forge-agent.yml
curl -fsS http://127.0.0.1:29190/api/v1/targets
```

To verify delivery without taking a production dependency offline, send a synthetic canary through Alertmanager and inspect the durable local record:

```sh
canary_ts="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
curl -fsS -X POST http://127.0.0.1:29193/api/v2/alerts \
  -H 'content-type: application/json' \
  --data "[{\"labels\":{\"alertname\":\"MonitoringDeliveryCanary\",\"severity\":\"info\",\"service\":\"forge-monitoring-canary\"},\"annotations\":{\"summary\":\"alert delivery canary\"},\"startsAt\":\"$canary_ts\"}]"
tail -n 1 .runtime/monitoring/alerts.jsonl
```

## Operate

Prometheus, Alertmanager, Grafana, exporters, and the user services all have restart policies. `systemd --user` must remain enabled for the user session (`loginctl enable-linger axia`) if this host must continue running the Forge services without an interactive login. The current deployment remains loopback-only and uses a local JSONL sink; external paging, TLS termination, backup shipping, and multi-host federation are separate production work.

To stop the deployment:

```sh
docker compose --env-file .runtime/monitoring.env -f infra/monitoring/compose.yaml down
systemctl --user disable --now erp-agent-monitoring-health.timer erp-agent-alert-sink.service erp-agent-api.service erp-agent-worker@{1..8}.service
```

Do not remove the named volumes or `.runtime/monitoring/alerts.jsonl` during a routine stop; they contain monitoring history and alert evidence.
