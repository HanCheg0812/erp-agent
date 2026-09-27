# Implementation Status

2026-09-27 当前 Cube 主机部署与长期监控：`erp-agent-api.service`、8 个 `erp-agent-worker@.service`、30 秒健康快照 timer 和本地告警 sink 已切换为持久用户级 systemd 服务，`loginctl Linger=yes`。Prometheus 2.54.1、Alertmanager 0.27.0、Blackbox/Node/PostgreSQL exporters 和 Grafana 11.2.0 通过 Compose 启动，全部端点 loopback-only；Prometheus 11/11 targets 为 `up`（含 Cubelet 原生 `/v1/metrics/resource`），14 条规则通过 `promtool`，Alertmanager synthetic canary 按 30 秒聚合延迟写入脱敏 JSONL，API 重启后 ready、metrics 和 8 个 worker 均恢复。完整证据、端点和限制见[监控部署记录](monitoring-deployment.md)。这证明当前 Cube 主机持久运行和告警投递，不代表公网生产、高可用、远端通知、备份恢复、guest 工作负载指标或长期业务负载验收。

2026-09-26 Cube/Forge 模板切换接线：新增 `POST /v1/cases/{case_id}/sandbox/switch`，由 Cube 继续负责模板与 Sandbox 生命周期，Forge 负责已采纳模板校验、同一 PostgreSQL 会话的第二运行租约、resident Bridge/JCode 配置、原子提升和旧实例释放。切换期间消息入队与 Worker provisioning 会拒绝；替换失败会清理非活动实例并保留旧活动实例。PostgreSQL 新增 `agent_session_runtime_leases`，`case_sandboxes` 增加 `active/session_lease_id`，并保留旧绑定的模板、实例和审计记录。系统 fixture 已通过成功切换、会话保持、旧租约撤销、切换后消息和失败回滚；这不是 Cube 原地换模板，也不是生产部署验收。详情见[模板接入计划](runtime-management.md#2026-09-26-forge-aware-template-switch)。

2026-09-26 Cube 浏览器模板：`cube_browser_browse` 已在真实常驻 JCode 回合中使用独立 Chromium 模板读取 `https://example.com`，标题/正文标识正确，结果和一次审计已落 PostgreSQL，工具实例删除已核实；工具 4,191 ms、总回合 6,642 ms。`npm run check` 通过。仅一个受限域名样本，不代表通用浏览器、并发或故障恢复验收。原生 Cube Web 的新建沙箱模板选择也已实际核对；现有实例没有原地换模板入口，原生页面本身不会自动建立 Forge/JCode 会话绑定，现由 Rust API 的受控切换路由补齐这条业务边界。之前的管理页实验已把本地 release 切为 version 2 / `tpl-e30d9fe2d99f47cf963459db`，不能继续沿用下文历史记录中“默认未切换”的状态。实验页面仍非接受方向。命令、失败链路、报告及限制见[浏览器验收记录](runtime-management.md#2026-09-26-browser-template-acceptance)。

2026-09-26 Cube 多模板工具复验：`cube_quantity_summary` 通过四轮真实 JCode/Cube 对话，结果、唯一标记回忆、PostgreSQL 审计及独立工具实例删除均正确；第二轮后销毁常驻实例，后两轮从同一 PostgreSQL 会话恢复。工具耗时 447-615 ms；伪造 token 与浏览器 token 均被内部工具端点以 401 拒绝。现有常驻模板另通过四会话冷/暖两轮测试，8/8 回复正确，总响应 P95 为 3,807/1,872 ms；这不是代码工具并发测试。Web 1440/390/320 px 与刷新检查通过，Cube WebUI 和八个 Worker 运行中；557 次资源采样无抓取错误，但 guest 指标仍缺失，工具时间线尚缺安全参数/结果摘要及耗时。报告根目录 `/tmp/erp-cube-quantity-acceptance-20260926/`；模板候选只用于 canary，默认 release 未切换。先前 `npm run check`、Bridge 单测及故障 fixture 通过，本轮仅修改验收脚本和文档。代码工具并发、浏览器/解析模板、历史监控/告警与跨节点回收仍未验收；当前主机监控已在 2026-09-27 独立部署并记录，长期业务负载与跨节点回收仍未验收。命令、证据及限制见[模板接入计划](runtime-management.md#2026-09-26-follow-up-acceptance)。

2026-09-26 Cube 官方接入建议核对：已读取官网模板、LangChain 工具接入、自定义镜像及 Pi 常驻预热文档，依据与项目选择记录在[模板接入计划](runtime-management.md#official-guidance-review)。官方示例支持 Harness 常驻模板或工具调用另一模板实例；envd 适用于 SDK 命令/文件接口，自有 HTTP 服务可以使用应用协议。代码核对确认当前 Bridge 在首轮才构建 JCode，模板 READY 不能作为完整 Agent 会话预热的证据。此轮仅补充计划与差距记录，额外模板仍未接通，未修改运行代码或重启服务。

2026-09-26 当前设计重点：用户明确希望 JCode 能复用 Cube 的其他模板环境。按[模板接入计划](runtime-management.md)区分“JCode 所在的兼容模板”和“受控工具调用的其他模板实例”；后者无需每个都内置 JCode。已核对 Cube 官方源码中的代码、浏览器及基础模板目录，但额外模板的工具适配尚未实现。当前处于 Shape plan，下一步固定一个有界操作及其权限、协议、生命周期与验收契约；保留尚未提交的管理页实验，不继续扩展其功能。本次澄清仅修改计划，未重启服务或改变运行中的模板选择。

2026-09-26 受控环境读取：按用户要求保持本地临时探针开启，默认配置仍关闭。真实 Cube 冷回合完成五条固定只读命令，PostgreSQL 工具原始结果确认 `/home/forge/work`、`forge` UID 10001 和 Ubuntu 24.04.5；暖回合只转述已有结果，不能算第二次执行。Web 以“读取运行环境”展示五条完成记录，刷新和 1440/390/320 px 检查通过。命令、会话标识、开关生效时机和证据见[常驻运行时记录](cube-resident-runtime.md)。

2026-09-26 当前模板 50 会话复测：在 `tpl-02d04202aa634f329df8d19f` 上用八个 Worker 运行合成短提示，冷/暖两轮各 50/50 完成，100 次均走 Cube、无工具调用，目标模板和独立 session/实例已核对。冷/暖总耗时 P95 为 4,524/2,100 ms；50 个测试实例均释放。逐条核对保存回复后，算术正确率为冷轮 46/50、暖轮 47/50；7 条错误回复不能被完成率掩盖，且没有宿主机同条件对照，尚不能归因于 Cube。500 ms 资源采样的峰值 PostgreSQL 连接数为 120、`host_sandbox` 序列为 51（含已有实例），未见内存限额失败；Cube guest 工作集指标仍未启用。命令、原始报告、限制见[常驻运行时记录](cube-resident-runtime.md)。

2026-09-26 本地常驻模型切换与 Web 复测：同一 Cube/JCode 会话第一轮选择 `deepseek-v4-pro` / `none`，第二轮选择 `deepseek-flash` / `max` 时，先前实现只更新思考程度，PostgreSQL `agent_sessions.metadata.model` 仍为 Pro。Worker 现在把每轮模型写入受控 Bridge 请求，Bridge 校验模型并在常驻 JCode 上切换 provider；新镜像 `127.0.0.1:5000/forge-agent-runtime:model-turn-20260926` 导入 Cube 模板 `tpl-02d04202aa634f329df8d19f`，应用探针 READY，规格 750m CPU / 1024 MiB。重启本地 API 和八个 Worker 后，`/ready` 为 200，真实浏览器脚本 `PLAYWRIGHT_MODULE=/tmp/erp-agent-flow-check/node_modules/playwright/index.mjs VERIFY_CONTROLS_OUTPUT=/tmp/erp-agent-controls-20260926 node --env-file=.env scripts/verify-web-agent-controls.mjs` 通过四组检查：真实 ERP 订单列表/明细与未配置 OpenViking 的失败事件、1440/390/320 宽度的输入框和模式菜单、Pro/关闭思考/完全访问设置持久化，以及同会话 Flash/最高切换与停止恢复。第二轮 JCode PostgreSQL 会话元数据实测为 `deepseek-flash` / `max`，20 张截图和报告见 `/tmp/erp-agent-controls-20260926/`，浏览器 page error 为零。Bridge 6 项单测、完整 `npm run check` 与 `git diff --check` 通过。本次只验证本地单会话模型切换；不代表 50 用户真实混合负载、OpenViking 检索、ERP 写入、解析入库或生产部署验收。

2026-09-25 Cube 身份与工具边界：旧模板实际运行 UID 0，不能只凭 OCI `USER forge` 判断隔离；入口脚本使用 `setpriv` 降到 UID/GID 10001 后，新模板真实冷/暖五条固定只读命令的 PostgreSQL 工具结果均为 `forge`。关闭临时 Bash 开关并重启本地服务后，冷/暖复测均为零工具调用。原始报告分别在 `/tmp/erp-cube-acceptance/nonroot-probe-live-20260925/report.json` 和 `/tmp/erp-cube-acceptance/bash-disabled-live-20260925/report.json`；细节见 [常驻运行时记录](cube-resident-runtime.md)。这不是通用 Bash 或 ERP 写权限的开放。

2026-09-24 RAG embedding 基础：新增独立 `services/ingestion-worker` 的 TEI HTTP 客户端和 `embedding-worker` 二进制。PostgreSQL 新增 `knowledge_chunks`、带租约/fencing token/退避的 `knowledge_embedding_jobs`，以及 pgvector `knowledge_chunk_embeddings`；worker 校验文本 SHA-256、模型登记维度并幂等 upsert，绝不消费个人 `memory_outbox`。迁移已在当前 pgvector 本地数据库执行，ingestion-worker 的 9 个单元测试与 5 个 fixture 测试通过。尚未接入真实 TEI、MinerU 产物入队或 OpenViking 发布，因此不把此项称为端到端 RAG 验收。

2026-09-24 本地 TEI 部署配置：Compose 新增独立 `embedding` profile，默认 CPU 镜像、固定 BGE-M3 revision、持久模型缓存及 localhost 端口；另有针对 RTX 50 系列的 GPU 覆盖配置。worker 现在读取 `.env`，启动时核对 TEI 实际模型与 PG 登记值。CPU/GPU Compose 配置校验、worker 编译与 14 项测试通过。本机 GPU 已识别，但 Docker 尚缺 NVIDIA GPU runtime（`--gpus all` 设备驱动不可用）；首次 `npm run embedding:up` 在 GHCR 镜像分层下载缓慢时停止，容器尚未创建，也未下载模型。真实 `/health`、`/info`、`/embed` 和 worker 到服务的链路仍待验证，不把 Compose 配置当成已运行的推理服务。

2026-09-24 Cube resident worker opt-in：Rust API 新增案件级 resident 运行路径。设置 `AGENT_EXECUTION_BACKEND=cube` 后，首条消息透明创建并等待案件绑定的 Cube 实例，向 guest Runtime Bridge 发送 `/v1/turn`；后续消息复用同一 PostgreSQL/JCode session、案件 MCP capability 和 HMAC runtime token。默认 local worker 不变，Cube 释放/回收时撤销相关租约。`cargo check` 与 API 测试通过；真实 resident 模板尚未注册，因此不把此项称为 Cube 端到端验收。

2026-09-24 ERP MCP capability session boundary：新增 PostgreSQL `mcp_sessions`，支持 planner 创建无业务任务的短期 capability，也支持 Worker 为单次运行创建任务绑定 capability；网关按 capability 前缀路由，调用时重新执行 EBAC、全局/任务范围、平台用户 ERPNext 绑定和 ERP 结果校验。恢复运行会撤销旧 capability 后重新申请，主动撤销和 15 分钟过期均拒绝后续调用。隔离系统 fixture 已验证无案件创建、ERP 读取、撤销后 401，以及原有任务范围、ERPNext 403 和 Agent guidance；未执行 ERP 写入。

2026-09-24 ERP MCP Phase A 真实双用户权限交集：在本地 ERPNext v16.35.0 真实 HTTP API 上，用 `planner@example.test -> forge-reader@example.test` 和同租户 `erp-boundary-limited@example.test -> limited-reader@example.test` 完成只读联调。forge 通过 Amazon + `SH-04-GREY-XL` 读到 `SAL-ORD-2026-00003`；limited 通过 Temu + `SH-04-GREY-L` 仅读到 `SAL-ORD-2026-00001`，列表/明细均返回统一 envelope，审计中的 `erp_user` 与 `trace_id` 可对应。limited 读取 Shopee `SAL-ORD-2026-00002` 时 ERPNext 原生 403 被转为 `erp_forbidden`（`retryable=false`、`safe_next_step=ask_erp_admin`）；平台显式 deny 返回 `platform_forbidden`；案件客户超出平台 grant 返回 `erp_scope_missing`。请求使用合成用户/订单，未执行 ERP 写入；正式 Vault/KMS provider、撤销/过期轮换、生产 ERPNext 尚未验收。可重复脚本见[真实 ERP 权限边界验证](../../../../scripts/validate-real-erp-boundary.mjs)。

2026-09-24 ERP MCP Phase A 第一片：ERP `list_erp_orders` / `read_erp_order` 统一返回 `status`、`as_of`、`trace_id`、`sources`、`data`、`warnings` envelope；审计事件写入同一 `trace_id`，来源只保留 Sales Order 的公开标识与 `modified`，ERP 原始响应仍由适配器字段白名单裁剪。系统 fixture 已覆盖 envelope、来源和审计 trace 关联；API 53 项单元测试、3 项 HTTP 测试及完整 `npm run check` 通过。此项只覆盖 ERP 只读切片，OpenViking envelope、正式凭据 provider、撤销/过期轮换和生产 ERPNext 仍待下一步验收。

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

- Current Phase: Rust API、个人认证、RustFS 原件、用户隔离、真实上下文压缩与 PostgreSQL 恢复已完成本地验证；当前主机 API/worker/Cube 探针及 Prometheus 告警链路已持久部署，Cube 常驻 JCode 继续接真实解析和知识服务。
- Last Landed: 2026-09-22 Rust/Axum API、Argon2id 邮箱注册、RustFS 私有对象、压缩组件及迁移校验；并发 worker、失败恢复、Skill 版本归档和 DeepSeek Responses Loop 已通过。流式正文在显示前写入 PostgreSQL 检查点，断流、主动停止、Runtime 强杀、API 重启和超过 150 秒的受控回合已验证；默认不再设置 150 秒总时限。
- Next Target: 按 [解析设计](parser-pipeline-design.md)，基于已接通的 RustFS 上传实现持久文档任务、MinerU/视觉适配和统一产物，再接 OpenViking 与 Rust MCP 网关。
- Last Verified: `npm run check` 通过 Web 类型检查、Rust 构建、PostgreSQL 隔离集成测试和 Web 构建；2026-09-27 当前 Cube 主机监控 11/11 targets 为 `up`、14 条规则通过校验、Alertmanager canary 已落盘，Cube systemd 状态进入 Prometheus，API 重启恢复和 Linger 已核实。新案件真实手动压缩从 17,491 降至 3,292 estimated tokens，61 条原始消息保留并在 API 重启后恢复事实，RustFS 原件哈希/越权拒绝/重启持久化通过。1440/390/320 登录、注册、记忆、压缩组件截图和新账号隔离检查通过；证据见 [Rust 验收](rust-auth-context.md) 与[监控部署记录](monitoring-deployment.md)。Bridge compaction regression、解析 fixture 与既有 Skill/Cube/业务证据见各联调记录。
- Context policy: 会话预算使用当前模型的上下文窗口，不再由 Forge 固定到 200k。DeepSeek API-key `/models` 不提供窗口值；当前支持的 `deepseek-flash` 和 `deepseek-v4-pro` 依据官方资料映射为 1,048,576 tokens，目录中若提供窗口值则优先。`AGENT_COMPACTION_SOFT_RATIO` / `AGENT_COMPACTION_CRITICAL_RATIO` 默认 0.80 / 0.90，按预算比例触发后台/紧急压缩。运行桥启动时会拒绝与模型窗口不一致的预算。压缩只改变发给模型的活动上下文，PostgreSQL 继续保留原消息和检查点。80%/90% 阈值单元测试、运行桥构建、API 49 项测试、MCP 构建和前端类型检查已通过；自动压缩的真实链路已用 2% 软阈值完成触发、事件、持久化、重启恢复验收，80% 实际长上下文负载仍未消耗约 80 万 tokens 做压力验证。
- Blocked By: 模型配置已具备。Cube 内常驻 Harness 和双案件隔离已通过本地探针；真实 OpenViking/TEI/MinerU 链路、正式凭据、ERP 受控写入、长时间混合负载及公网生产/高可用部署仍未验收。当前主机的 loopback-only 持久监控不等于生产级告警。

## Active TODO

1. 按 [解析设计](parser-pipeline-design.md)固定 MinerU/视觉模型接口，实现真实 ParseBackend 和带原文位置的统一产物，以双类合成图片对比解析效果。
2. 让解析 Worker 从 RustFS 原件生成 `knowledge_chunks` 并入队，接入真实 TEI 后再接 OpenViking 发布及 Rust MCP 网关；验证引用、隔离、失败恢复，并启用相应业务 Skill。
3. 按[常驻运行时记录](cube-resident-runtime.md)补齐 guest 工作集和长时间混合负载测量，对照外部 Harness；重建恢复、双案件隔离和暂停恢复已有本地证据。持续观察当前监控栈并补齐外部通知、备份和故障演练。
4. 接入 ERPNext，补齐批准后 ERP 状态重验、受控执行及对账；与知识依据合并验收。

API 的登录 `sessions`、Agent 存储 `agent_sessions`、案件输入 `case_conversation_messages` 是不同记录。配置模型后，新消息事务性排入 `agent_runs`，JCode 执行模型/工具循环；无配置时只保存输入，后续配置不会补跑旧消息。审批不执行 ERP 写入。

本地 API：`http://127.0.0.1:3100`，Web 开发入口见 README。未登录首页只提供邮箱登录/注册；`View demo` 合成入口已移除。有效登录刷新保留；退出及服务端 401 返回登录页并清空账户界面，跨标签页同步的历史验证见 [Rust 验收](rust-auth-context.md)。新账号从空工作区创建首个案件，本地开发凭据在 `.env`。生产部署未做。

Mindloop 专属资源已按授权清理，实测回收约 164 GB，Cube 和其他项目保留，详见[清理记录及明确保留项](mindloop-cleanup.md)。
# 2026-09-24 云端记忆与 OpenViking 基础

个人记忆的第一版云端化已落地：`user_memories` 兼容保留原 API，同时增加 `scope_kind/scope_id`、`version`、`category`、生命周期状态、来源哈希、有效期和生成式词法列；API/Agent 写入会在同一 PostgreSQL 事务内写入 `memory_audit_events`，个人 embedding outbox 只有在显式开启 `PERSONAL_MEMORY_EMBEDDINGS_ENABLED=1` 时才创建，删除改为 `retracted` 并按同一开关产生 `retract` 任务。Worker 已按当前消息执行 exact、substring 和 `simple` `tsvector` 召回，限制 12 条，并在查询时重新限定租户、用户、作用域、有效期与 active 状态。Compose 已切换并实测 `pgvector/pgvector:pg17-bookworm`，`pg_trgm`、`vector` 扩展和三项记忆迁移在现有 PostgreSQL 卷上成功执行；记忆事务/幂等/容量/隔离/撤回断言通过系统 fixture，Rust API 单测 57 项及 HTTP 测试 3 项通过。

当前尚未把这项基础实现写成完整 RAG 验收：个人记忆没有启用 embedding 模型，文档 RAG 的 `bge-m3` 仅作为登记示例，OpenViking 仍只接收企业知识工具请求，个人记忆不投影到 OpenViking。完整系统回归本轮在 Cube fixture 的固定 `test-sandbox` 唯一键冲突处中止，不能据此宣称全仓 `npm run check` 通过；这不影响上述记忆断言已经在同一回归中的通过结果。
