# Skill 与解析基础联调

日期：2026-09-22。范围：本机合成案件、专用 DeepSeek Responses、PostgreSQL 和 Rust 测试。未调用真实 ERP、MinerU、视觉解析或 OpenViking，未部署生产。

## 本轮落地

`skills/business/` 提供四份版本化业务指引。Rust Bridge 构建时嵌入完整目录快照，运行时在私有目录复用 JCode 的 SKILL.md parser，阻断开发者目录导入。新增 `skill_find` 与只读 `skill_manage(list/load/read)`，仍由现有 JCode Loop 自行选择调用；允许的工具共四个，包括 `case_read` 和 `todo`。加载指引不会开放额外工具，也不能审批或写 ERP。

当前需求变更指引可加载；证据阅读、图表核对和 ERP 核查可被发现，但因对应工具尚未接入而返回缺失依赖，不返回指引正文。发现支持中英文关键词，没有额外 embedding 或在线安装功能。压缩事件已经落到 Rust API 的 agent_runs，并由 Web 展示前后 token、保留消息、摘要长度、节省量和耗时。

迁移 `20260922020919_create_skill_catalogs.sql` 增加目录快照归档、每轮目录哈希和已加载 Skill 版本。API 校验目录与正文 SHA-256，加载事件必须匹配该轮目录；完整加载结果保存在 JCode 的 PostgreSQL 消息中。Skill 源文件不进入普通 Web conversation API，页面显示实际工具步骤与模型回复。一个回合使用固定构建快照，后续回合可通过重建 Bridge 切换版本并分别留档；中断运行不会自动重放。真实长上下文压缩和进程恢复已通过，发布撤回管理仍未实现。

`services/ingestion-worker` 新增 Rust 库：`ParseBackend` 定义能力、模型参数校验、提交、轮询、取消和可序列化远程句柄；配置支持单父继承、抽象基类、对象递归覆盖、数组替换。切换后端会清除旧模型与参数，必须重新通过兼容性校验。原件保留不可关闭，最终校验必须依赖全部已启用解析阶段。展开配置使用规范化 JSON / SHA-256，重试标识绑定租户、业务修订、输入、配置、步骤与实验编号。

解析部分目前是库和 fixture 测试，尚无可运行解析 Worker、真实解析适配器或文档队列。原件上传与 RustFS 私有存储已在后续 Rust API 阶段接通，见 [Rust/认证/压缩验收](rust-auth-context.md)。远程句柄序列化不等同于解析进程故障恢复验收。

## 真实失败与修复

增加 Skill 后，模型同轮返回两个工具调用，后续 DeepSeek 请求出现 400。检查原生响应和请求构造后发现两处问题：JCode 未持久保留 DeepSeek 的 `reasoning_text` 内容；历史归一化将同轮调用排成 `call A -> result A -> call B -> result B`，使相邻的 assistant 调用被工具结果隔开。

补丁保留原生 reasoning 内容，经 PostgreSQL 序列化恢复后回传；同轮连续函数调用先排列完整，再追加对应结果，异步完成顺序不改变调用匹配。旧 OpenAI encrypted/summary 载荷仍按原格式处理，不添加空的 DeepSeek content。流式 reasoning 与最终 reasoning item 不重复显示。协议依据：[DeepSeek Responses 指南](https://api-docs.deepseek.com/guides/responses_api)、[创建响应契约](https://api-docs.deepseek.com/api/create-response)。

另发现 JCode 自动为工具参数补充 `intent` / `accept_large_output`。Skill 的严格输入解析已接受这两个有类型的共享字段，仍拒绝未知字段、任意路径、安装和 reload。未以自动重试掩盖失败；两个失败合成案件保留，修复后另建案件验收。

上游指针仍为 `2a4edaa02057ac994a601311c4f03ed450e1b3c9`，相关补丁位于本地子模块，未提交或更新远端。

## 验证结果

| 检查 | 结果与边界 |
| --- | --- |
| `npm run check` | 类型检查、21 项真实 PostgreSQL 测试、API 构建通过；新增目录校验、版本绑定、去重和恢复覆盖 |
| `npm run build:web` | Web 构建通过，四个工具具有业务操作标签 |
| `npm run parser:test` | 5 项 fixture 测试通过：继承/切换/哈希、非法配置、校验依赖、实验隔离、异步句柄；未测真实模型 |
| `npm run agent:test` | Bridge 的 Skill 权限与发现边界测试通过 |
| `cargo test --locked --manifest-path services/runtime-bridge/Cargo.toml -p jcode-provider-openai -j 6` | 26 项 provider 测试通过，包括原生 reasoning 恢复和并行函数排序；Rust Bridge 已重新构建并实际运行 |
| 四份 SKILL.md | Skill creator 的结构校验通过 |
| 浏览器 | 1440x1000、390x844、320x844 截图检查，无页面横向溢出，输入框在视口内；无 pageerror |

尝试通过 Bridge workspace 运行 `jcode-provider-openai-runtime` 的额外测试时，Cargo 因该依赖不属于当前 workspace 且需要 dev-dependencies 而拒绝启动；不计为通过。实际链接的 runtime 已随 Bridge 编译，并完成下述真实工具循环。未构建或验收整套上游 TUI。

真实案件 `f6a18f12-a750-49fe-913a-1e6858a61b19`，会话 `session_2926f85f611d468aacd2cd1330eb6109`：

1. 模型自行调用 `skill_find`、`case_read`，取得结果后调用 `skill_manage`，加载 `demand-change-review@1.0.0` 并完成回答；确认 1280 是替换后的总量，明确缺少 ERP 基线与库存依据。
2. 停止并重新启动 API 后，在同一案件追问。新 Bridge 从 PostgreSQL 恢复核对代号 `MAPLE-731` 与数量口径，重新加载同一 Skill 并读取案件，回答完成。
3. 两轮 `agent_runs` 都为 completed，目录哈希均为 `82bdaa9a3d7e3a4b564237a2d8415f0452d934b0b9af10e221a1f80a1d68ce8a`，各自记录 Skill 版本及正文哈希；数据库只有一份对应目录归档，消息存储中可查询到 Skill 正文工具结果。
4. 运行中临时目录只有发布的 Skill 副本及 JCode 运行标记，未发现 session/todo JSON/JSONL 会话文件。

两轮从浏览器提交后轮询观察到完成约 7646 ms / 5602 ms，包含排队、进程启动、模型和轮询开销；不是 TTFT、冷暖对照或容量基准。

本机临时证据：`/tmp/erp-skill-live/report.json`，`desktop-flow.png`、`mobile-390-flow.png`、`mobile-320-flow.png`；脚本 `/tmp/erp-agent-flow-check/skill-live.mjs`。浏览器首轮脚本曾因 exact-text 定位包含状态子节点而失败，修正定位后复用已完成案件验证，未重复发送首轮请求。

## 下一步

固定 MinerU 与视觉模型接口及合成双类图片，实现真实适配器和可定位的统一产物；在现有上传基础上接 PostgreSQL 文档任务和发布。随后接入 Rust MCP 网关与 OpenViking 范围检索，评测原文定位、版本隔离与 RAG 质量。ERPNext 实时工具、Cube 内 Harness 与混合负载均保持独立验收；主机进程的压缩恢复已在后续 Rust API 阶段通过。
