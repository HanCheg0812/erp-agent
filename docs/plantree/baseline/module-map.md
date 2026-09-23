# 模块

当前目录与职责见 [根 README](../../../README.md)。API 的 auth/cases/approvals/audit 及共享契约已实现，包含迁移、测试与锁文件；JCode 源码位于 external/jcode。Web 已有 React/TypeScript 变更案件工作台，构建与合成数据交互已验证，真实 API 联调尚待验收，见 [Web 记录](../plans/server-demo/web-ui.md)。runtime-bridge、mcp-gateway、ingestion-worker 的集成状态单独跟踪。

架构依据见 [完整方案](../../architecture/erp-agent-demo-architecture.md)。逻辑模块不要求各自部署为独立微服务。
