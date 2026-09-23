# Rust 后端、个人账户与上下文

日期：2026-09-22。状态：核心切换已验证；真实 ERP、知识服务和生产部署仍未验收。

用户最新要求覆盖之前保留 TypeScript API 的决定：项目自建后端统一 Rust，Web 保持 React/TypeScript。ERPNext、OpenViking、MinerU 使用上游原生服务，不重写第三方软件。

## 实施边界

1. 在同一 PostgreSQL 数据模型上迁移身份、案件、审批、审计、会话存储、模型队列与 Cube 生命周期，保留接口及历史记录。Rust API 已通过兼容性、重启、隔离检查并替换 Fastify 入口；当前运行二进制为 `services/api/target/debug/erp-agent-api`。
2. 个人账户采用邮箱和密码注册/登录，注册创建个人租户。密码使用 Argon2id，兼容已有 scrypt 账号；服务端生成身份和角色，不接受客户端指定 owner/tenant/roles。未接邮件服务前不声称邮箱已验证。
3. 案件授权同时检查 tenant 与 owner；会话凭据绑定用户、案件与会话，失效及续租仍由 PostgreSQL 控制。审批继续绑定版本、动作摘要和幂等键。
4. RustFS 使用本项目专用卷、凭据、端口及桶；原件与解析产物的路径由后台生成，下载前校验文档所属用户和案件。会话、压缩状态与个人记忆仍在 PostgreSQL。
5. 沙箱按用户和案件会话分配，不能把一个可写 HOME 给同一用户的多个案件复用；可预热无用户数据的模板。快照不回滚 ERP 写入。
6. 接出 JCode 真实压缩事件，在聊天中展示触发原因、前后 token 估计及保留消息数量。用合成资料测试摘要保留、tool/result 配对及进程恢复，不把缩短输出当成上下文压缩。
7. 上游 JCode 的长期 memory 默认写 global/project JSON，不能直接打开用于多用户服务。个人记忆走用户范围的 PostgreSQL 数据与受控注入；企业资料仍属于 OpenViking RAG，案件事实不自动升级为跨案件个人记忆。

## 验收

- 邮箱注册、重复/并发注册、登录、退出撤销、旧账号兼容、跨用户访问拒绝。
- 案件与精确版本审批、重复写入、JCode CAS/fencing、队列与 Cube 对账的行为保持。
- 真正压缩成功后的持久化、进程重建与事实恢复；失败不能显示成功，历史不能丢失。
- RustFS 私有对象上传/读取、内容校验、越权拒绝与重启持久化。
- 桌面与窄屏的认证及压缩组件检查，灰白视觉和原有聊天流程保持。

## 已观察结果

- `npm run check` 通过：Web 类型检查、Rust 构建、PostgreSQL 业务/并发/恢复测试和 Web 构建均通过。新增压缩跨用户拒绝、幂等、操作类型冲突、失败和非法指标拒绝、重启指标恢复测试。失败任务显示专门的压缩重试提示，不伪装成成功结果。
- 新建合成案件的真实 DeepSeek Responses Loop 完成案件只读和个人记忆读取；手动压缩把 17,491 降到 3,292 estimated tokens，减少约 81.2%，摘要 2,073 字符，保留 10 条近期消息，耗时 6,643 ms。61 条原始消息全部保留在 PostgreSQL，51 条被摘要覆盖。API 进程重启后正确恢复 QUARTZ-914、1280、2026-10-22、修订 C、delivery-note-R3 page 2 table 1，以及“替换总量、未写 ERP”的语义。这是单次合成样本，不代表生产 P95 或自动压缩验收。
- RustFS 私有 PNG/TXT 原件通过内容哈希、幂等、跨用户拒绝和服务重启后的读取验证。手动重启本项目 RustFS 容器前后运行 `scripts/verify-rustfs-restart.mjs`，报告写入 `/tmp/erp-rust-live/rustfs-restart.json`；脚本本身不执行重启。
- Playwright/Chrome 检查了 1440、390、320 宽度的登录、注册、个人记忆和真实压缩面板，共 12 张截图，无页面或面板横向溢出、输入框超出视口、pageerror。通过浏览器注册个人测试账号，案件和记忆初始为空；已有演示账号重新登录能看到自己的真实压缩结果。独立聊天检查确认没有“沙箱就绪”或“Agent 未连接”业务横幅。

## 复现与证据

- 新案件：`91fbaa41-abf6-4925-bf6f-3178c271ac70`；会话：`session_f23b0cc2116e44269b6323912afce0fa`。完整验收在 2026-09-22 04:10:50 UTC 结束。
- 真实模型报告：`/tmp/erp-rust-live-clean/report.json`；测试脚本 `scripts/verify-rust-live.mjs` 按次保留报告和脱敏 API 日志。旧压缩测试中的失败记录保留；旧日志已被早期脚本覆盖，不能从通用报错确定其具体原因。新案件首次读取、短会话无须压缩、长会话压缩、重启追问均连续成功。
- UI 报告：`/tmp/erp-live-integration/auth-compaction-ui.json`；截图：`final-{login,register,compression,memory}-{1440,390,320}.png`。浏览器探针：`/tmp/erp-agent-flow-check/auth-compaction-ui.mjs`。
- 真实验收命令：`LIVE_ACCEPTANCE=1 LIVE_ACCEPTANCE_OUTPUT=/tmp/erp-rust-live-clean node --env-file=.env --env-file=.runtime/deepseek.env scripts/verify-rust-live.mjs`。需要项目模型配置、RustFS、PostgreSQL 和已编译 API/Bridge；运行前停止同库的其他 Agent worker，避免任务被其他进程领取，完成后恢复本地 API。该脚本会创建合成案件、测试账号和私有原件，并调用真实模型。
- 上游回归 `messages_for_provider_applies_manual_compaction_in_native_auto_mode` 已通过，覆盖手动压缩后的 provider 历史选择及压缩 manager 与当前 session 一致；Bridge 通过 Agent 的该接口访问 manager。

以上是本机合成数据和本地服务的验收边界，不代表 ERPNext、OpenViking、MinerU 或生产部署已经接通。

## 登录入口修正（2026-09-22）

用户反馈在 `http://127.0.0.1:4173/` 看到 Northwind 演示，而之前截图来自已登录账号。原因是 Web 未登录时默认进入 Demo，登录控件藏在侧栏底部；服务端 401 也仅显示连接错误，没有清除失效登录。之前登录弹窗通过测试，不能证明首次进入应用的流程正确。

- 未登录首页改为独立邮箱登录/注册页面；`View demo` 明确进入演示，演示侧栏提供登录与退出入口。
- 注册成功进入个人空工作区，可以直接创建首个案件；手机空页面补上账户侧栏入口。
- 统一 API 请求和文件下载的 401 处理；旧账户请求返回时不能清除新的登录。切换账户卸载之前的案件、对话和记忆组件。
- 退出撤销 Rust 服务端会话并清理本地登录，同一浏览器其他标签页同步；过期会话回到登录页，暂时性 API 故障保留登录并允许重试。
- Playwright/Chrome 在实际 4173 入口完成 10 项检查：首次进入、错误密码、显式 Demo、真实注册与空记忆、创建案件及刷新恢复、新标签页恢复及退出撤销、使用中 401、重载失效 token、503 重试、账户切换与跨账户案件拒绝。报告 `page_errors` 为空。
- 检查并保存 1440/390/320 登录与注册页面，以及 390x400 矮视口、手机空工作区、手机/桌面真实案件、过期提示，共 11 张截图，无横向溢出。矮视口按可滚动页面检查；不声称已验收真实手机软键盘。
- 修正后 `npm run typecheck`、`npm run build:web` 均通过，运行中的 Rust API `/ready` 返回 200。

复现：`PLAYWRIGHT_MODULE=/tmp/erp-agent-flow-check/node_modules/playwright/index.mjs node scripts/verify-web-auth.mjs`。脚本需本地 Rust API、Web、Chrome 和 Playwright；创建合成测试账号与案件，不访问真实模型或 ERP。报告 `/tmp/erp-auth-entry/report.json`，截图在同目录。完成时间 2026-09-22 04:47 UTC。邮箱验证、找回密码及生产发布仍未完成。
