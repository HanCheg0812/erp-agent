# Agent 批准模式调研

调研日期：2026-09-22。用户指出输入框应提供“请求批准 / 帮我批准 / 完全访问”，随后要求先调研。本记录区分已核对事实与实施建议；三种模式已在本项目 Rust API/JCode Loop 中实现，JCode 原生 Harness 权限协议仍未直接启用。

## 结论

此前实现的 `chat / read` 是工具访问范围，不符合用户要求的批准模式，应在后续实现中更正。已撤回未注册、未应用的批准模块及 migration 016 草稿；未修改已运行数据库。

JCode 存在权限协议、SDK 自动同意、ambient 批准队列和工具前置 hook，但当前项目没有可直接启用的完整三模式。保持 JCode Agent Loop，在 Rust 业务工具执行入口补充批准控制，比移植其 TUI 或另写 Agent Loop 更符合当前架构。

## 三种模式的参考语义

按 [Codex Permissions](https://learn.chatgpt.com/docs/permission-modes)、[Sandbox](https://learn.chatgpt.com/docs/sandboxing) 和 [Auto-review](https://learn.chatgpt.com/docs/sandboxing/auto-review)：

| 模式 | 谁处理需要批准的动作 | 原本已允许的操作 |
| --- | --- | --- |
| 请求批准 | 用户 | 正常执行，不逐次询问每个读取 |
| 帮我批准 | 独立审核 Agent，能够允许或拒绝 | 正常执行；不会因代审扩大沙箱范围 |
| 完全访问 | 不再逐次请求批准；Codex 中同时移除本地沙箱限制 | 直接执行，仍受组织及实际系统权限约束 |

Codex 的参考配置分别为 `workspace-write + on-request + reviewer=user`、`workspace-write + on-request + reviewer=auto_review`、`danger-full-access + never`。这只是参考产品的含义，不是 JCode 已支持的配置字段，也不能直接把文件系统沙箱模式等同于本产品的 ERP 数据授权。

“帮我批准”改变审核者；不能用“读取一律允许”或“所有请求自动回复 allow”冒充。审核 Agent 的输入应包含用户任务与已授权范围、实际工具及参数、目标资源与副作用；任务 Agent 不应自行批准自己的请求。

## JCode 代码核对

项目固定来源：`1jehuang/jcode`，commit `2a4edaa02057ac994a601311c4f03ed450e1b3c9`。未升级 submodule。

| 能力 | 核对结果 | 对当前项目的影响 |
| --- | --- | --- |
| `PermissionRequest / PermissionResponse` | Harness API 有类型，决策包含 allow / allow_always / deny | 有协议类型不代表当前运行路径会发送请求 |
| Harness API bridge | `permission_response` 明确返回 InvalidRequest；hello 不声明 permissions | 固定版本及本次读取的上游 master 均未接通该能力 |
| SDK `autoApprove` | 收到 permission_request 后直接回复 allow，且要求服务端声明 permissions | 不是独立模型审核；当前 bridge 不发事件，开关不能补齐能力 |
| ambient `request_permission` | 限定 ambient 会话；SafetySystem 管理批准队列与决定 | 当前业务 Agent 直接调用 Rust Agent，未注册该工具；现有队列使用 JSON 文件，不符合本项目 PostgreSQL 唯一持久存储约束 |
| `/autoreview` | `AutoReviewConfig` 明确用于回合结束后的代码审查，TUI 触发 reviewer | 不是工具执行前的权限审核，不能借同名开关实现“帮我批准” |
| `pre_tool` hook | 工具执行前调用；exit 2 拒绝，exit 0 允许；其他错误及超时放行，默认超时 5 秒 | 可参考拦截位置；不适合原样充当业务批准的强制执行边界 |

代码依据：

- [请求与决策类型](https://github.com/1jehuang/jcode/blob/2a4edaa02057ac994a601311c4f03ed450e1b3c9/crates/jcode-harness-api/src/requests.rs#L111)、[事件类型](https://github.com/1jehuang/jcode/blob/2a4edaa02057ac994a601311c4f03ed450e1b3c9/crates/jcode-harness-api/src/events.rs#L180)。
- [固定 bridge 的不支持说明](https://github.com/1jehuang/jcode/blob/2a4edaa02057ac994a601311c4f03ed450e1b3c9/crates/jcode-harness-api-server/src/translate.rs#L966)；[本次读取的上游 master](https://raw.githubusercontent.com/1jehuang/jcode/master/crates/jcode-harness-api-server/src/translate.rs)。后者可变化，不代表升级验收。
- [Rust SDK 能力检测](https://github.com/1jehuang/jcode/blob/2a4edaa02057ac994a601311c4f03ed450e1b3c9/crates/jcode-sdk/src/client.rs#L548)、[自动同意](https://github.com/1jehuang/jcode/blob/2a4edaa02057ac994a601311c4f03ed450e1b3c9/crates/jcode-sdk/src/client.rs#L1264)。[官网 SDK 文档](https://jcode.sh/sdk)展示权限 API，但实际支持仍需检测服务端 capability。
- [ambient 工具](https://github.com/1jehuang/jcode/blob/2a4edaa02057ac994a601311c4f03ed450e1b3c9/crates/jcode-app-core/src/tool/ambient.rs#L644)、[SafetySystem](https://github.com/1jehuang/jcode/blob/2a4edaa02057ac994a601311c4f03ed450e1b3c9/crates/jcode-base/src/safety.rs#L150)。[Safety 文档](https://github.com/1jehuang/jcode/blob/master/docs/SAFETY_SYSTEM.md)标记为 Design，不能当作完整运行验收。
- [AutoReviewConfig](https://github.com/1jehuang/jcode/blob/2a4edaa02057ac994a601311c4f03ed450e1b3c9/crates/jcode-config-types/src/lib.rs#L966)、[TUI 自动审查触发](https://github.com/1jehuang/jcode/blob/2a4edaa02057ac994a601311c4f03ed450e1b3c9/crates/jcode-tui/src/tui/app/commands_review.rs#L900)。
- [Registry 工具前置拦截](https://github.com/1jehuang/jcode/blob/2a4edaa02057ac994a601311c4f03ed450e1b3c9/crates/jcode-app-core/src/tool/mod.rs#L884)、[hook 超时和错误语义](https://github.com/1jehuang/jcode/blob/2a4edaa02057ac994a601311c4f03ed450e1b3c9/crates/jcode-config-types/src/lib.rs#L934)。

## 面向本项目的实现

1. 三种模式共用已发布业务工具目录。ERP/OpenViking 工具是否存在与连接是否配置不应由批准菜单决定；批准模式控制需要确认时由谁决定。保留 `case_read`、todo、Skill 查找/加载；不因此暴露通用 Bash。
2. 授权范围内的 ERP 查询、知识检索正常执行。需要额外批准的具体动作进入 Rust 批准控制；不让模型自己选择是否经过检查，也不只依赖 MCP 的 readOnly 标签。
3. “请求批准”在聊天中显示待执行动作、目标和参数，支持批准/拒绝并继续原工具调用。“帮我批准”由独立、无业务写入权限的审核调用返回结构化决定与简短理由；拒绝或审核失败不自动转为允许。“完全访问”省去 Agent 的逐次确认，但不能授予其他用户数据访问权或越过后端角色权限。
4. ERP 的精确版本业务审批与 Agent 工具批准分别建模。完全访问不默认为任意订单提供业务签字；仍保留既定的提案版本、ERP 状态复核及幂等规则。界面说明应明确“完全访问”的范围是当前账户已授权工具与数据。
5. PostgreSQL 保存请求、原始工具调用标识、参数指纹、用户/会话范围、决定来源及状态。暂停、刷新、取消、进程重启和重复批准必须能恢复且不重复执行。
6. 实施前处理当前 MCP HTTP 20 秒超时与人工等待的冲突；确定挂起/恢复契约，避免把长时间等待当作工具失败后重新执行。审核模型可复用专用模型服务，但审核上下文与执行权限独立；延迟、费用和误判需单独实测。

验收：已允许的读取不弹窗；同一待批准动作三种模式分流正确；拒绝/审核失败不会执行；改参后旧批准失效；跨用户不可代批；取消或重启后恢复不产生重复写入。当前回合的模式、操作工具、读取工具和业务审批角色由后端注入 JCode 系统上下文，`case_read` 返回同一授权对象，避免模型仅凭聊天历史猜测权限。详见[工具授权与业务审批](approval-workflow.md)。
