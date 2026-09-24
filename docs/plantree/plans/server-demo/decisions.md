# 已确认决策

日期：2026-09-21。来源：当前用户会话。

1. 使用 JCode 的 Rust Harness，自建 Web；不采用 OpenCode 作为产品运行时。
2. CubeSandbox 纳入首版。用户重新提出 JCode 在内或在外两种方式，要求调研官方建议；位置重新开放，当前建议优先验证沙箱内常驻实例，保留外部对照，见[调研](../../../architecture/cube-runtime-placement.md)。
3. 保留 todo；业务 Agent 不需要通用 Bash。受控 read、ERP MCP、知识库 MCP 构成主要工具。
4. 修改默认编码提示词，保留任务规划能力，注入业务案件、依据和审批状态。
5. ERPNext 用于真实 Demo；OpenViking 配合 MinerU，检索必须区分客户、物料与版本。
6. 批量解析采用异步 Worker；共享模型服务放在会话沙箱外。
7. 全链路考虑高性能，使用预热、连接复用、有界并发与队列，并以实测验证。

8. 数据库明确为 PostgreSQL，不需要 PostgREST。后端按业务领域分目录，例如 auth/。
9. JCode 源码纳入仓库并保留远端更新能力；采用固定 commit 的 submodule，不自动跟随上游。
10. JCode session 及消息、工具结果、todo、压缩/恢复状态统一持久化到 PostgreSQL；不保留 JSON/JSONL 文件作为正式来源或定期双写。见[会话存储约束与适配范围](../../../architecture/session-storage.md)。
11. Web 工作台与 Agent Flow 结合：案件为入口，Flow 展示工具调用与分析结果，工作台承接证据、方案和审批。先用可播放、暂停和重播的合成案例验证交互；模拟运行不代表真实 Agent 或 ERP 接入。
12. 用户再次确认 OpenViking 也通过 MCP 提供给 Agent。计划链路为 JCode → 受控 stdio 业务 MCP 网关 → 知识权限适配 → OpenViking HTTP MCP；ERP MCP 与知识 MCP 是工具依赖，不作为多个独立 Agent 展示。检索和原文读取的权限、客户/项目/料号/版本/有效期范围由后台实施。
13. 用户澄清 Flow 指类似 Chat 的 Agent 对话流，并指出 Astryx 已有相关组件。Web 首屏改为案件会话列表 + Chat；分析步骤、MCP 工具调用、证据入口和精确版本审批沿 Agent 回复呈现，单据/证据/活动按需在侧栏打开。节点图降为次级执行详情，替代第 11 条的工作台/节点图优先布局；后台架构不因此调整。灰白配色保持，先验证合成演示和截图。
14. 用户要求工具名称体现 ERP / OpenViking 下的具体操作，主界面不附加 MCP 后缀。MCP 是接入协议，Agent 通过受控网关调用有明确 schema 的业务工具。运行时仍为 JCode 模型与工具结果之间的循环，调用次数和顺序由模型及工具依赖决定；同轮多个调用也合法，不把演示的固定步骤当作产品编排。当前动画增加订单结果回到 Agent 后再检索依据的阶段，仍为固定合成脚本。聊天内容在输入区上方渐隐，末尾保留滚动空间，避免最终操作被渐隐遮住。

15. 2026-09-22：先完成业务 API、Web 和会话持久化，项目模型配置随后提供；当前消息保存不触发模型，也不制造 Agent 回复。Cube 同期接入案件生命周期并排查本地故障。
16. 用户要求不显示“沙箱就绪”“Agent 未连接”。这些状态及生命周期事件留在后台，业务界面仅显示消息保存、方案、审批与相关操作结果。
17. 用户授权移除当前不需要的 Mindloop 专属运行时及相关资源，保留 Cube 和其他项目；清理范围、证据及混用目录保留项见[记录](mindloop-cleanup.md)。
18. 用户提供专用 DeepSeek 配置，要求直接接原生 Responses 格式，取代第 15 条的模型配置暂缓状态。由现有 JCode Rust Agent 执行工具循环，Bridge 调用其 Agent 接口；不另写模型循环，也不改走 Chat Completions。仅新输入触发运行，历史保存消息不补跑。
19. 用户要求增加思考过程组件和正文打字效果。按模型真实返回的思考片段和工具事件顺序展示，可展开/收起；正文平滑渐进显示、完成后移除光标，刷新历史不重播，并尊重减少动态效果设置。
20. 用户重申 RAG 效果是项目核心，开始考虑 OpenViking 底座及图片上传/解析；首批图片明确为“两类都有”：需求单/表格截图/扫描文件，以及产品照片/零件图/工程图纸。后续重点转向资料解析、正确范围检索与原文引用。[接入方案](rag-foundation.md)中的模型、接口、实施批次和量化评测仍是待验证建议，不能把已有 Chat 接通视为 RAG 已完成。
21. 2026-09-22：用户确认 Skill 作为业务引导，保留/恢复 JCode 的 Skill 发现能力，并扩展项目业务 Skill，与 ERP/OpenViking 工具组合。固定上游版本提供 `skill_manage` 而非独立 `skill_find`；现已通过项目 Rust 包装恢复发现/加载并新增查找扩展，权限和审批仍由后端实施。见 [Skill 设计](skill-tool-design.md)及[实际验证](skill-parser-integration.md)。
22. 新增部分能用 Rust 尽量用 Rust，优先覆盖 MCP 网关、解析 Worker 和运行时扩展。已有 Node/TypeScript API 保持运行；MinerU/OpenViking/模型服务使用其原生服务接口，暂不重写。具体模块分工见 [Skill 设计](skill-tool-design.md)。
23. 解析应支持复用父级配置、组合多个处理阶段与替换不同解析模型，方便后续测试。采用 Rust trait/组合、单父配置继承及可追溯的父子产物，区分业务修订和解析运行；详情见 [解析设计](parser-pipeline-design.md)。trait、配置和依赖校验已实现，父子实际产物和模型组合尚待接入实测。

24. 用户要求自建后端全部使用 Rust，覆盖第 22 条保留 TypeScript API 的决定；同时新增个人邮箱注册登录、RustFS 对象存储、用户隔离和 JCode 压缩测试及前端组件。沙箱、长期记忆与会话压缩分别处理，具体迁移及验收见 [Rust 后端与上下文](rust-auth-context.md)。

25. 用户要求会话入口不再强制填写业务案件字段，输入框参照提供的 Chat 样式，并暴露模型端点、模型选择、思考程度及权限控制。后续明确权限应为“请求批准 / 帮我批准 / 完全访问”，不是 `chat/read` 工具范围；最新指示先调研，见[批准模式调研](approval-modes-research.md)。三模式后台与独立审核 Agent 尚未实现。
26. ERP 与 OpenViking 应作为两个 MCP 服务注册并暴露各自领域工具；用户确认 Embedding 可以后续配置。ERP 读取已实际联通，OpenViking 工具注册与接口适配已完成，真实检索仍等待服务与 Embedding 配置。
27. 2026-09-22：用户指出 SQL 迁移应采用时间前缀而非单纯递增编号，并要求现有迁移全部修改。现有 `001`–`018` 已统一改为 `YYYYMMDDHHmmss_<change_description>.sql`，UTC、精确到秒、用途使用 snake_case，详见[迁移规范](../../../../services/api/migrations/README.md)。SQL 内容和执行顺序不变，但 `schema_migrations` 的持久化 ID 已变化；迁移器会在 checksum 一致时于同一事务内把旧 ID 改为新 ID，不重复执行历史 SQL。执行器按 `MIGRATIONS` 注册顺序运行，按注册 ID 与 SQL checksum 判断是否已应用。
28. 2026-09-22：用户要求 DDD 开发按文件目录驱动，而不是按根目录函数文件扩张。Rust API 以 `src/<domain>/mod.rs` 作为领域边界，当前包括 `auth`、`cases`、`conversation`、`agent_sessions`、`sandboxes`、`integrations`、`objects`、`permissions`、`platform` 与 `worker`；`main.rs` 只组装路由和运行时生命周期，跨领域调用通过模块接口完成。
29. 2026-09-22：用户进一步指出 `mod.rs` 和 handler 集中承担业务职责的问题。`mod.rs` 只声明/导出，handler 只适配 HTTP；用例负责权限、事务与编排，领域规则负责校验与状态迁移，repository 负责 SQL/锁/持久化。禁止仅把大 handler 换成大 service。案件核心及后续会话、工具权限、原件/记忆已按此拆分；Agent 记忆通过所属模块用例加入权限事务。其余模块直接 SQL 和共享 API 错误耦合如实记录，不将其称为全项目 DDD 完成。见[实现与证据](api-module-boundaries.md)。
30. 2026-09-23：用户要求所有模块都写相应测试。采用“纯规则直接单测，SQL/锁/事务用真实 PostgreSQL 集成测试，外部适配用受控 HTTP/S3/Cube/ERP/OpenViking/Runtime fixture”的分层方式，不为 repository 伪造无数据库单测。全部 10 个顶层 API 模块已登记测试证据，Rust 直接测试 45 项，测试矩阵本身由架构测试防遗漏；详见 [API 测试矩阵](../../../../services/api/TESTING.md)。
31. 2026-09-23：按 Rust/Cargo 与 Axum 社区惯例，以服务为测试归属。API crate 增加 `src/lib.rs` 作为公开可测试服务入口，导出 `router(App)` 和生命周期；`src/main.rs` 仅委托 library。私有规则测试留在源码模块，服务级 Rust 集成测试放 `services/api/tests/`，共用支持代码放 `tests/support/mod.rs`，跨服务/多进程验收脚本迁到仓库级 `tests/system/`。第 28 条中 `main.rs` 组装路由的描述由本条更新。
32. 2026-09-23：工具保留稳定的内部 ID 用于 JCode 调用、MCP 路由和审计；对话步骤与批准卡片显示简短的业务动作名称，例如“查看附件”“查看订单”“搜索业务资料”，不展示 `mcp__...` 前缀。新增工具若暂未配置展示名，界面使用通用动作标题，不能因此导致会话页崩溃。此约定参考 OpenCode 将工具 ID 与界面动作标题分开的做法。

33. 2026-09-23：用户确认 ERPNext 原生用户/角色/单据权限与平台 EBAC 互通。EBAC（本项目暂定义为 Enterprise Business Access Control）只管平台用户是否能让 Agent 执行业务工具及其任务/资源范围；ERPNext 继续作为 ERP 单据权限来源。每次 ERP MCP 调用取两边权限交集。当前数据库中的 `cases` 仅作为兼容命名，业务语义改为业务任务/变更事项，不把 ERPNext 改造成案件或财务系统。详细设计见 [ERP MCP 与 EBAC](erp-mcp-ebac.md)。

34. 2026-09-24：先落地工具级 EBAC，不引入完整平台角色同步。`integration_grants` 增加工具、动作、资源类型、效果、策略版本和有效状态；MCP API 入口执行具体工具的 allow/deny，显式 deny 优先。旧的无 `tool_id` grant 只兼容现有四个只读工具，不能开放未来工具或写入动作；平台用户到 ERPNext 用户绑定和 ERPNext 原生权限交集作为下一步。

35. 2026-09-24：身份绑定按租户固定为平台用户到 ERPNext 用户的一对一有效绑定。新增 `erp_connections` 与 `erp_identity_bindings`；本地演示绑定 `forge-reader@example.test` 使用受限的 `ERP_CREDENTIAL_FORGE_READER` 环境凭据，`shared_demo` 仍需显式开关和明确绑定。生产仍需 Vault/KMS provider，两个不同 ERPNext 用户的原生权限交集尚未验收。

36. 2026-09-24：ERP MCP 后续按三阶段推进：先完成正式凭据、MCP capability 生命周期和两个非管理员 ERPNext 用户的只读权限交集；再实现固定 DocType/字段边界的业务推演工具；最后以 proposal、审批、ERP 状态重检、幂等和对账为前置条件，选择一个草稿类写工具做端到端验收。写工具在此之前不注册给业务 Agent。详见 [ERP MCP 下一阶段设计](erp-mcp-next-phase.md)。

实施选择：Rust API 已通过本地兼容性、隔离、重启和压缩检查并成为运行入口；React/TypeScript Web 保持，思考组件参考 Astryx ChatReasoning，正文使用 `@astryxdesign/core@0.6.2` 的 `useStreamingText`，保持灰白视觉。OpenViking 和 Cube 保留独立服务边界；真实知识检索、解析适配器和混合负载仍待验收。
