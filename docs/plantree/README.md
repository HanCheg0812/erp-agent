# 项目计划入口

当前模式：ERPNext 原生权限、平台 EBAC、业务任务与 ERP MCP 网关的本地只读链路已验证；当前主机的 API、worker、Cube 探针和 Prometheus/Alertmanager/Grafana 已完成 loopback-only 持久部署，详见[监控部署记录](plans/server-demo/monitoring-deployment.md)。下一阶段按正式凭据、业务推演、受控写入分阶段推进。真实 MinerU 持久解析和 OpenViking 检索尚未接入，Cube 仍需 guest 资源指标与长期混合负载验收。

权威顺序：用户最新决定 → [已确认决策](plans/server-demo/decisions.md) → [架构方案](../architecture/erp-agent-demo-architecture.md) → 路线图。实际运行能力必须以验证记录为依据。

| Plan | Status | Current Phase | Last Landed | Next Target |
| --- | --- | --- | --- | --- |
| [服务器 Demo](plans/server-demo/README.md) | In Progress | 当前主机持久监控已部署；ERP/RAG 与受控写入待接入 | 2026-09-27 当前主机 API/worker/Cube 探针及 Prometheus 告警链路实测 | 正式 ERP 凭据边界、真实解析/知识检索、写入重验及长期混合负载 |

先读 [基线](baseline/README.md)，再读计划及其路线图。低承诺想法进入 [ideas](ideas/inbox.md)。
