# 服务器 Demo

状态：In Progress。场景限定民品生产运营。

当前交接见 [implementation-status](implementation-status.md)，包含已验证能力和最多四项近期任务。

输入框的“请求批准 / 帮我批准 / 完全访问”现在按会话持久化到 PostgreSQL；切换会立即 PATCH 当前会话，下一轮由后端注入权威权限上下文，运行中的回合拒绝切换。通用工具授权与业务审批见[通用工具授权与业务审批](approval-workflow.md)。流式中断修复见[原因与验收](stream-recovery-fix.md)。

Web Agent Chat 与灰白视觉的实现、验证及边界见 [Web 会话记录](web-ui.md)。

2026-09-22：[Web/API 与 Cube 生命周期联调](web-api-integration.md)通过真实浏览器、PostgreSQL 和进程重启检查；[Mindloop 清理](mindloop-cleanup.md)完成并保留 Cube。

随后按用户提供的专用模型配置完成 [DeepSeek Responses 与 JCode 联调](deepseek-integration.md)，包含真实工具循环、todo、重启恢复、可展开思考过程和正文打字效果。

后续推进 [OpenViking RAG 接入](rag-foundation.md)：用户确认首批图片兼有扫描/表格资料和照片/图纸，以解析准确、检索范围正确、原文可定位和对比评测为重点。当前先完成 [ERP MCP 与 EBAC](erp-mcp-ebac.md) 的身份与权限边界；RustFS 原件上传和下载已接通，真实解析和知识检索尚未实现。

最新 [Rust 后端、个人账户与上下文](rust-auth-context.md)已完成核心验收：自建 API 切换为 Rust，邮箱注册、个人记忆、用户隔离、真实上下文压缩及 PostgreSQL 重启恢复已验证。Web 保持 React/TypeScript。

[开源实践调研](rag-references.md)记录 MinerU 官方 Skill / Document Explorer、社区 OpenViking + DeepSeek 问答、官方集成和 RAG-Anything，重点借鉴按需深读、图表上下文及可定位引用；案例阅读不等于本地运行验收。

按 [Skill 发现与 ERP/知识工具组合](skill-tool-design.md)接入四份版本化指引和受控发现/加载工具，真实模型与 API 重启验证通过；[可替换解析流水线](parser-pipeline-design.md)的 Rust trait、配置继承、阶段依赖与结果标识已实现并通过 fixture 测试。真实解析后端和 Worker 尚未接入，具体证据与 DeepSeek 连续工具调用修复见 [Skill/解析基础联调](skill-parser-integration.md)。

目标：以真实运行的 ERPNext 和合成资料，演示客户需求变更、影响分析、人工确认、受控写入及执行跟踪。ERP MCP 的身份、工具授权、读取边界和短期 capability 已在本地合成 ERP 与真实双用户只读链路中验证；共享演示身份不代表个人 ERPNext 权限验收，写入仍关闭。

阅读顺序：[决策](decisions.md) → [路线图](roadmap.md) → [待确认信息](open-questions.md)。详细设计见 [架构](../../../architecture/erp-agent-demo-architecture.md)，模型行为见 [Prompt 草案](../../../../prompts/erp-agent-system-prompt.md)。

当前新增身份与权限设计：[ERP MCP 与 EBAC](erp-mcp-ebac.md)，以及配套的[ERP MCP 工具目录](erp-mcp-tools.md)。身份权限先于 ERP 写入实现；当前 ERP 只读工具已接入工具级 EBAC，支持无任务/任务绑定 MCP capability，并通过 `erp_connections` / `erp_identity_bindings` 绑定演示用户的个人凭据引用。正式凭据服务、目标生产 ERPNext 复核和受控写入仍待验收。下一阶段按[ERP MCP 下一阶段设计](erp-mcp-next-phase.md)推进只读生产化、业务推演和受控写入。

业务基础已实现：分域 API、PostgreSQL、身份、案件、方案版本、审批幂等、事务审计、Web 对话保存与 Cube 实例生命周期。JCode 源码已固定引入，其 PostgreSQL 存储补丁在真实模型循环与主机进程重启中通过验证。Cube 内外两种方案的[调研记录](../../../architecture/cube-runtime-placement.md)已落盘；OpenViking 真实检索、Cube 常驻 Harness 或生产部署验收仍未完成。

2026-09-22：Rust API 源码改为目录驱动的领域模块。`services/api/src` 下每个领域以目录和 `mod.rs` 作为边界，避免继续按根目录函数文件扩张。2026-09-23 按 Rust 服务测试惯例进一步将路由和生命周期移入 `lib.rs`，`main.rs` 只保留 executable 委托。

后续按用户反馈完成 [HTTP 与业务职责拆分](api-module-boundaries.md)：8 个业务 HTTP 适配模块不再直接访问数据库或外部服务，案件、会话、工具权限和原件/记忆模块分开用例、规则与 repository。19 项单元/边界测试及本地 PostgreSQL 集成回归通过（原件使用受控 S3 fixture）；其他模块仍有直接 SQL 等后续边界工作，未进行生产部署。

2026-09-23：按用户要求补齐全部顶层 API 模块的对应测试，并按服务归属整理位置。当前 46 项模块/架构测试及 3 项 `services/api/tests/` Router 集成测试通过；PostgreSQL 与受控外部服务的跨进程 fixture 已迁至 `tests/system/` 并完整回归通过。逐模块证据见 [API 测试矩阵](../../../../services/api/TESTING.md)。
