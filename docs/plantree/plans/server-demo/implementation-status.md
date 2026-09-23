# Implementation Status

2026-09-23 Web 入口调整：按当前个人演示范围移除未登录页 `View demo` 及浏览器内合成案件、固定回复、模拟工具调用和模拟审批分支；只保留真实登录/注册与账户会话。前端 `npm run typecheck` 和 `npm run build:web` 通过；在本地 4174 浏览器确认未登录只出现登录表单，切换后显示注册表单。此项不改变后端案件、方案、审批及 ERP/MCP 权限边界；生产部署未做。

2026-09-23 多进程 Agent 分发补充：`agent_runs` 每条消息一行，同一案件维持单活，不同案件由多个独立 Rust worker 同时消费；HTTP 进程可关闭内置 worker。事务提交后的 PostgreSQL `NOTIFY` 唤醒所有 worker，5 秒扫描兜底，迁移注册遗漏已修复并加 SQL 文件覆盖测试。50 个独立用户/案件、2 秒固定 runtime 的一次本地探针中，1/2/4/8 个 worker 各 8 槽的完成 P95 分别为 12.49/6.40/4.32/2.54 秒，50/50 均完成；8 worker 的实际峰值为 50。此为模拟 Runtime 分发测量，不是 DeepSeek 或 Cube 业务容量。Cube 官方预热建议强调真正的 Agent session readiness 和明确的 session 池/每沙箱任务模型；详情及命令见 [Agent 并发探针](agent-runtime-concurrency.md)。

本地 3100 服务现为 1 个无内置 worker 的 API + 8 个独立 worker，每个 8 槽（总 64）。八条 PostgreSQL 监听连接、`/ready` 和切换后的一条真实 DeepSeek Flash 短回复已验证；隔离数据库的 8×8、50 用户固定 2 秒 Runtime 探针完成 50/50，完成 P95 冷轮 2.54 秒、暖轮 2.24 秒。此前本机 4×4、50 用户模拟探针两轮均完成，P95 6.35/6.28 秒，作为对照。完整 `npm run check` 通过。该本地服务为用户级临时 systemd unit，重启机器后需重新启动；真实模型 50 用户和 Cube 常驻路径尚未测。

2026-09-23 Agent 并发补充：Rust API worker 从逐案等待改为每进程有界并发，`AGENT_MAX_CONCURRENT_RUNS` 默认 4、允许 1-8，保留跨进程 PostgreSQL 案件锁和停止恢复。消息及批准事务提交后即时唤醒本进程 worker，任务结束也即时扫描，周期扫描仅作恢复兜底。PostgreSQL 系统测试用 2 秒固定 runtime fixture 实测 1/2/4/8 案件，峰值分别为 1/2/4/4；空闲批次排队 P95 为 7/16/79 ms，8 案件批次约 4.17 秒完成，超过上限的请求进入下一批。已记录每批排队、首字及总耗时，见 [Agent 并发探针](agent-runtime-concurrency.md)。该结果不含真实多路 JCode/DeepSeek/Cube；Cube guest 到宿主业务 API 的连通探针失败，常驻 JCode 仍未接入。

2026-09-23 Cube 基础创建并发探针：用现有 2 vCPU / 2 GiB lightweight 模板，对空闲健康节点做 1/2/4/8 路各 3 轮并发创建、健康检查和自动释放。12 轮共 45 个实例全部创建且健康，观测到并发创建响应 P50/P95：1 路 62/63 ms、2 路 75/77 ms、4 路 61/77 ms、8 路 78/83 ms；端到端代理健康 P95 最高约 92 ms。每轮等待约 0.89-1.01 s，主要包含约 0.8-0.9 s 的逐实例删除确认。测试中主机 MemAvailable 从约 40.0 GiB 降至 39.7 GiB，结束后全部带标记实例清零。探针和逐请求原始数据见 [Cube 并发记录](cube-concurrency.md) 与 [机器可读证据](cube-concurrency-evidence-erp-agent-cube-bench-e7618602-ab7a-4161-ae7d-d2457bc4fab1.json)。此结果只覆盖基础模板的并发创建及健康探针，不包含 JCode/Bridge、模型生成、活跃会话或稳态/饱和压力，不能据此承诺业务并发容量。

2026-09-23 附件会话页面故障：在 4173 的真实浏览器中复现登录后空白页。API 和静态资源均为 200；控制台指出 `ThinkingProcess` 对新工具 `attachment_read` 解构未定义的显示标签而崩溃。Web 已登记该工具标签，并对未来未知工具名使用原名和通用图标兜底。用含 PDF 页图及 `attachment_read` 记录的会话刷新后，附件、工具步骤、回复和输入框均正常显示。本次前端崩溃与此前请求字段/空游标故障是不同层的问题。

2026-09-23 聊天附件联调：`+` 菜单可直接将文件/图片上传到当前案件的 RustFS 原件，输入框展示待发附件并支持移除；消息可只含附件，发送后保存有序引用，历史可下载。Rust API 在事务内绑定附件并复核隔离、状态、总量与幂等；Worker 从 RustFS 校验 SHA-256 后将 PNG/JPEG/WebP 输入 JCode，PDF/DOCX/XLSX/MD/TXT 由 Rust 提取有界文本与可用图片。新增受当前案件/用户/会话限制的 `attachment_read`，通过偏移、页码和图片游标续读并记录审计。独立进程真实 DeepSeek Flash 图像探针正确读出合成 PNG 上的 `7392`；真实长 Markdown 对话已调用 `attachment_read` 并正确读出第二段代号。解析器和系统回归覆盖 PDF/Word/Excel/文本样本、长文本续读和跨案件拒绝。旧 API 进程拒绝新字段、Bridge 透传空游标两处联调故障已修复并复测。真实模型逐格式文件问答、MinerU 解析、OpenViking 索引、跨设备 UI 与多图负载仍待验证。详情见[附件设计及验证](chat-attachment-agent-design.md)。输入框 `+` 菜单不再含手动“压缩上下文”；自动压缩和占用提示保留。

2026-09-23 本地预览补充：移除侧栏会话状态下拉框及可见的 Workspace 文案，保留搜索、刷新、待审入口和账户操作；输入框旁的上下文窗口提示显示已用比例、估算已用与模型窗口总量。一次会话页面出现 `Request failed (500)` 时，3100 端口无 Rust API 监听，Vite 代理返回错误；重新启动后 `/ready` 为 200，测试账号下 48 个案件的会话历史请求均为 200，浏览器刷新后原消息恢复。旧进程没有留下退出状态或崩溃日志，不能确认退出原因；本地 API 现由用户级 transient systemd 服务 `erp-agent-dev-api` 托管并设置失败重启，重启机器后仍需重新启动。完整 `npm run check` 通过；没有进行生产部署。

2026-09-23 自动压缩验收补充：真实 DeepSeek 对话在独立 API 进程中以 2% 软阈值触发 JCode `reactive` 压缩，从 35,701 降至 21,565 estimated tokens，压缩 52 条、保留 11 条活动消息；PostgreSQL 中 63 条原始消息完整保留。修复了单回合 runtime bridge 在回答结束后退出、可能丢弃后台摘要任务的问题；bridge 现在等待已启动的摘要完成、持久化状态并上报前端压缩事件。API 重启后，模型从摘要恢复了 checkpoint、总量、交期、修订版、证据位置、非增量和 ERP 未执行状态。2% 只用于独立验收进程，正式默认仍为 80%/90%；本轮没有 ERP 写入。

2026-09-23 测试补充：10 个顶层 Rust API 模块均已登记对应的直接测试或受控集成证据，见 [API 测试矩阵](../../../../services/api/TESTING.md)。API 已形成 library + thin binary；46 项模块/架构测试和 3 项服务级 Router 集成测试通过，仓库级 `tests/system/` 跨进程测试及完整 `npm run check` 通过。这不是逐行覆盖率声明，也不代表真实外部服务或生产部署验收。

2026-09-22 结构补充：[API 职责边界](api-module-boundaries.md)记录 handler 拆薄，以及案件、会话、权限、原件/记忆分层；19 项单元/边界测试与 PostgreSQL/S3 fixture 回归通过。auth、agent_sessions、integrations、sandboxes 等剩余 repository 抽离尚未完成，不能将本轮当作全项目 DDD 验收；既有业务下一步不变。

2026-09-22 最新补充：[MCP/输入框联调](mcp-composer-integration.md)已完成 ERPNext 实际只读查询、两个 MCP 服务注册、模型/思考程度与停止的真实检查。OpenViking 工具已注册，服务及 Embedding 待配置。批准三模式和流式中断修复已实现，实测证据及限制见[最新验收](stream-recovery-fix.md)。

- Current Phase: Rust API cutover verified；个人认证、RustFS 原件、用户隔离、真实上下文压缩与恢复已落地，继续接真实解析和知识服务。
- Last Landed: 2026-09-22 Rust/Axum API、Argon2id 邮箱注册、RustFS 私有对象、压缩组件及迁移校验；并发 worker、失败恢复、Skill 版本归档和 DeepSeek Responses Loop 已通过。流式正文在显示前写入 PostgreSQL 检查点，断流、主动停止、Runtime 强杀、API 重启和超过 150 秒的受控回合已验证；默认不再设置 150 秒总时限。
- Next Target: 按 [解析设计](parser-pipeline-design.md)，基于已接通的 RustFS 上传实现持久文档任务、MinerU/视觉适配和统一产物，再接 OpenViking 与 Rust MCP 网关。
- Last Verified: `npm run check` 通过 Web 类型检查、Rust 构建、PostgreSQL 隔离集成测试和 Web 构建；新案件真实手动压缩从 17,491 降至 3,292 estimated tokens，61 条原始消息保留并在 API 重启后恢复事实，RustFS 原件哈希/越权拒绝/重启持久化通过。1440/390/320 登录、注册、记忆、压缩组件截图和新账号隔离检查通过；证据见 [Rust 验收](rust-auth-context.md)。Bridge compaction regression、解析 fixture 与既有 Skill/Cube/业务证据见各联调记录。
- Context policy: 会话预算使用当前模型的上下文窗口，不再由 Forge 固定到 200k。DeepSeek API-key `/models` 不提供窗口值；当前支持的 `deepseek-flash` 和 `deepseek-v4-pro` 依据官方资料映射为 1,048,576 tokens，目录中若提供窗口值则优先。`AGENT_COMPACTION_SOFT_RATIO` / `AGENT_COMPACTION_CRITICAL_RATIO` 默认 0.80 / 0.90，按预算比例触发后台/紧急压缩。运行桥启动时会拒绝与模型窗口不一致的预算。压缩只改变发给模型的活动上下文，PostgreSQL 继续保留原消息和检查点。80%/90% 阈值单元测试、运行桥构建、API 49 项测试、MCP 构建和前端类型检查已通过；自动压缩的真实链路已用 2% 软阈值完成触发、事件、持久化、重启恢复验收，80% 实际长上下文负载仍未消耗约 80 万 tokens 做压力验证。
- Blocked By: 模型配置已具备。尚未验证 ERP/知识服务、双案件模型运行、混合负载或 Cube 内常驻 Harness。

## Active TODO

1. 按 [解析设计](parser-pipeline-design.md)固定 MinerU/视觉模型接口，实现真实 ParseBackend 和带原文位置的统一产物，以双类合成图片对比解析效果。
2. 实现文档持久任务与 Rust Worker，消费现有私有原件，再接 OpenViking 及 Rust MCP 网关；验证引用、隔离、失败恢复，并启用相应业务 Skill。
3. 按[存储设计](../../../architecture/session-storage.md)验证 Cube 重建恢复，再验证常驻 JCode/Bridge 的双案件隔离、暂停恢复与预热，对照外部 Harness。
4. 接入 ERPNext，补齐批准后 ERP 状态重验、受控执行及对账；与知识依据合并验收。

API 的登录 `sessions`、Agent 存储 `agent_sessions`、案件输入 `case_conversation_messages` 是不同记录。配置模型后，新消息事务性排入 `agent_runs`，JCode 执行模型/工具循环；无配置时只保存输入，后续配置不会补跑旧消息。审批不执行 ERP 写入。

本地 API：`http://127.0.0.1:3100`，Web 开发入口见 README。未登录首页只提供邮箱登录/注册；`View demo` 合成入口已移除。有效登录刷新保留；退出及服务端 401 返回登录页并清空账户界面，跨标签页同步的历史验证见 [Rust 验收](rust-auth-context.md)。新账号从空工作区创建首个案件，本地开发凭据在 `.env`。生产部署未做。

Mindloop 专属资源已按授权清理，实测回收约 164 GB，Cube 和其他项目保留，详见[清理记录及明确保留项](mindloop-cleanup.md)。
