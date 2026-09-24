# 项目计划入口

当前模式：优先完成身份与权限边界。ERPNext 原生权限、平台 EBAC、业务任务与 ERP MCP 网关的设计已落盘；ERP MCP 下一阶段按只读生产化、业务推演、受控写入分阶段推进。真实 MinerU 持久解析和 OpenViking 检索继续排在其后。

权威顺序：用户最新决定 → [已确认决策](plans/server-demo/decisions.md) → [架构方案](../architecture/erp-agent-demo-architecture.md) → 路线图。实际运行能力必须以验证记录为依据。

| Plan | Status | Current Phase | Last Landed | Next Target |
| --- | --- | --- | --- | --- |
| [服务器 Demo](plans/server-demo/README.md) | In Progress | ERP MCP 第一阶段进行中，结果 envelope 已落地 | 2026-09-24 工具级 EBAC、ERP 身份绑定、本地凭据 provider、Agent 友好错误和 ERP 只读结果 envelope | 正式凭据边界、MCP capability 生命周期及两个非管理员 ERPNext 用户的双边授权交集验收 |

先读 [基线](baseline/README.md)，再读计划及其路线图。低承诺想法进入 [ideas](ideas/inbox.md)。
