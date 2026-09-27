# 当前主机部署与长期监控

## 2026-09-27 当前主机实测

本轮把 CubeSandbox 控制节点上的 Forge API、8 个 Agent worker、健康快照、Prometheus、Alertmanager、Blackbox Exporter、Node Exporter、PostgreSQL Exporter 和 Grafana 部署为持久服务。Cubelet 原生资源指标由 Prometheus 直接采集，Grafana 提供 Cube 专用仪表盘；Forge API/Worker 是被监控的业务集成依赖。部署范围是本机运维，不是公网生产环境：所有监控 HTTP 端点绑定 `127.0.0.1`，Alertmanager 先投递到本机的脱敏 JSONL sink。

Prometheus、Grafana 和 Alertmanager 位于 Cube 控制节点运维层，不复制进每个 JCode/业务模板；模板切换或沙箱替换时，控制面仍保留同一套采集与告警历史。

部署使用的固定入口和服务如下：

| 组件 | 运行方式 | 端点/状态 |
| --- | --- | --- |
| Forge API | `erp-agent-api.service` | `127.0.0.1:3100`，私有 metrics 为 `127.0.0.1:3101` |
| Agent worker | `erp-agent-worker@1..8.service` | 8 个实例均为 `active` |
| 健康快照 | `erp-agent-monitoring-health.timer` | 每 30 秒写入 `.runtime/monitoring/textfile/forge_agent_health.prom` |
| 告警 sink | `erp-agent-alert-sink.service` | `127.0.0.1:29198`，写入 `.runtime/monitoring/alerts.jsonl` |
| Prometheus | Compose `prom/prometheus:v2.54.1` | `http://127.0.0.1:29190`，保留 30 天 |
| Alertmanager | Compose `prom/alertmanager:v0.27.0` | `http://127.0.0.1:29193` |
| Blackbox Exporter | Compose `prom/blackbox-exporter:v0.25.0` | `127.0.0.1:29115` |
| Node Exporter | Compose `prom/node-exporter:v1.8.2` | `127.0.0.1:29100` |
| Cubelet resource metrics | Cubelet system service | `127.0.0.1:9998/v1/metrics/resource`，空闲节点可为 200/空体 |
| PostgreSQL Exporter | Compose `postgres-exporter:v0.15.0` | `127.0.0.1:29187` |
| Grafana | Compose `grafana/grafana:11.2.0` | `http://127.0.0.1:13200` |

观测结果：

- `promtool check config` 和 `promtool check rules` 均成功，规则文件加载 14 条规则。
- Prometheus `/api/v1/targets` 的 11 个活动目标均为 `health=up`：Forge API、Cubelet resource、Forge HTTP（API/Cube Web/Cube API/CubeOps/Cubelet resource/告警 sink）、Node、PostgreSQL exporter 和 Prometheus。
- `up` 查询的 11 个结果均为 `1`，当前没有 firing 告警。
- Alertmanager synthetic canary `MonitoringDeliveryCanary` 以及带 `source=cube` 的 `CubeMonitoringDeliveryCanary` 均按配置的 30 秒 `group_wait` 投递到 sink，并在 `.runtime/monitoring/alerts.jsonl` 观察到 `status=firing`、`receiver=forge-alert-sink` 的脱敏记录。
- API `/health`、`/ready` 和私有 `/metrics` 均在 API 重启后恢复；metrics 显示 `forge_api_up 1`、数据库连接池上限 16、Cube backend available 1、resident configured 1。
- API 重启后 8 个 worker 保持 `active`；健康快照在 30 秒周期内继续更新，记录 API、worker、Cube 服务和 HTTP 探针状态。
- Node Exporter 以当前主机 UID/GID 读取用户拥有的健康快照目录，`node_textfile_scrape_error=0`；Prometheus 查询到 5 个 `forge_cube_service_active` 序列且值均为 `1`。
- `loginctl show-user axia -p Linger` 为 `yes`，因此用户级 systemd 服务具备退出交互登录后继续运行的启动条件。
- Alertmanager 的单实例 gossip 已显式绑定 `127.0.0.1:29094`；复核 `ss` 未发现 `*:9094`，监控栈没有因默认 cluster listener 暴露到局域网。
- Prometheus 容器重建后从命名卷回放 WAL 并达到 `/-/ready`，证明本机监控数据卷可恢复；这不是备份恢复验收。
- Grafana 管理 API 使用本地生成凭据验证通过，默认 `Forge Prometheus` datasource、`Forge ERP Agent` 和 `CubeSandbox Observability` dashboard 均已加载；匿名访问按预期返回 401。

## Cube 边界与告警覆盖

Cube WebUI 是 Cube 的运行管理界面，不是已验证的 Prometheus/Alertmanager 规则编辑器。当前告警组件与 Cube 同部署在控制节点旁侧，规则位于 `infra/monitoring/rules/forge-agent.yml`：`source=cube` 覆盖 Cube Web/API/Ops、Cubelet resource endpoint、Cube systemd 服务和 Cube sandbox 内存限制失败；`source=forge`、`source=storage`、`source=host` 分别覆盖 Forge 集成、PostgreSQL 和主机基础设施。Grafana 同时提供 `CubeSandbox Observability` 与 `Forge ERP Agent` 两个只读仪表盘。Alertmanager 的本地 sink 只保留脱敏后的 labels、annotations、状态和时间，不接收密钥或完整业务响应。

## 运维边界

当前是单主机、loopback-only 的 Cube 部署监控基线。还没有接入外部 Pager/邮件/飞书、TLS 反向代理、跨主机联邦、远端时序备份、告警日志轮转策略或故障演练后的 RPO/RTO 结论；这些完成前不能称为生产级监控或高可用部署。Cube guest 工作集指标在当前空闲节点没有序列，长期混合负载、真实模型成本和业务 SLO 仍需单独测量，不能由 `up=1` 推导。

部署和日常检查命令见 [`infra/monitoring/README.md`](../../../../infra/monitoring/README.md)。
