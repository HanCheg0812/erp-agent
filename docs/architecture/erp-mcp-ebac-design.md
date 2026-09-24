# ERPNext MCP 与 EBAC 设计

日期：2026-09-23，2026-09-24 更新实现状态。本文定义平台业务任务、平台 EBAC、ERPNext 原生权限和 MCP 网关之间的边界。它不代表个人凭据、写入工具或生产部署已经完成。

## 1. 术语和边界

本项目暂把 EBAC 定义为 **Enterprise Business Access Control**，即平台对企业业务操作的授权层。这是本项目的术语，不声称是 ERPNext 的内置模块或行业统一标准。

| 概念 | 负责什么 | 不负责什么 |
| --- | --- | --- |
| 平台身份 | 平台用户、租户、组织成员、平台角色 | 不替代 ERPNext 用户 |
| EBAC | 用户能否让 Agent 使用某个业务工具、处理某个业务任务、访问平台资料 | 不复制 ERPNext 的角色权限 |
| ERPNext 身份与权限 | ERPNext 用户、角色、Role Permission、User Permission、字段和单据状态 | 不知道平台 Agent 的对话或审批 |
| 业务任务 | 一次多轮分析、变更处理、附件、方案、审批和审计的容器 | 不是 ERPNext 的“案件”或 ERPNext 权限单位 |
| MCP session | 一次 Runtime 与业务 MCP 网关的连接和调用上下文 | 不是登录身份，也不是授权结果 |

当前数据库中的 `cases` 是历史命名。业务语义按“业务任务 / 变更事项”理解；它可关联一张或多张 ERPNext 单据，但不向 ERPNext 新增案件模型。后续可通过 API 展示名、事件名和新表名逐步迁移，暂不为了改名破坏已有会话和审计数据。

实现进度：`integration_grants` 已支持工具、动作、资源类型、效果、策略版本和有效状态；MCP API 入口已对当前四个只读工具执行工具级 EBAC，并保留旧 grant 的受限兼容读取。`erp_connections` 与 `erp_identity_bindings` 已实现按租户绑定平台用户和 ERPNext 用户；`bound_user` 已通过受限环境凭据 provider 读取 `ERP_CREDENTIAL_*`，`shared_demo` 仍需显式本地配置，ERPNext 原生权限交集和正式密钥服务仍待实现。

## 2. 双边权限的判定

每次 ERP 工具调用都必须通过下面的交集：

```text
平台用户身份
  -> EBAC：允许的工具、动作、租户、任务和资源范围
  -> ERP 身份映射：该平台用户对应的 ERPNext 用户
  -> ERPNext：角色、Role Permission、User Permission、字段权限、单据状态
  -> 业务适配器：固定 DocType、参数边界、任务关联、版本和幂等检查
  -> ERPNext API
```

任一层拒绝都拒绝调用。业务任务只能缩小本次操作范围，不能扩大 ERPNext 用户权限；模型传入的 `customer`、`company`、`doctype` 或记录名也不能绕过服务端注入的范围。

平台 EBAC 处理“这个人能不能让 Agent 做这件业务操作”，ERPNext 处理“这个 ERP 用户能不能访问或修改这条 ERP 记录”。两边允许范围取交集，而不是把平台授权转译成一个更高的 ERPNext 角色。

## 3. 身份映射和凭据

平台用户需要有明确的 ERPNext 身份映射。当前数据关系已固定为 `erp_connections` 和 `erp_identity_bindings`：

| 数据 | 关键字段 | 约束 |
| --- | --- | --- |
| `erp_connections` | tenant、连接名、base URL、auth mode、状态 | 只保存连接标识和能力，不保存明文密钥 |
| `erp_identity_bindings` | platform_user、erp_user、connection、credential_ref | 一个租户用户只有一个当前有效绑定 |
| 凭据引用 | `credential_ref`、版本、过期时间 | 当前本地 provider 只解析受限 `ERP_CREDENTIAL_*` 环境变量；生产应接入后台凭据存储，密钥不进入浏览器、模型、Cube 镜像或 MCP stdio 环境 |
| 业务任务关联 | task、ERP DocType、record name、观察版本 | 只保存引用和快照摘要，ERP 当前事实回 ERPNext 查询 |

**MVP 推荐按平台用户一对一绑定 ERPNext 用户**：平台管理员先在 ERPNext 创建/启用用户，再在平台建立绑定；平台不自动注册 ERPNext 账号，也不复制两边的角色。当前每个租户平台用户只能有一个绑定，绑定指向一个 ERP 连接；调用 ERPNext 时目标方案是使用该用户自己的 API 凭据。ERPNext 的角色、User Permission 和单据状态由 ERPNext 管理，平台管理员不能通过 EBAC 越过它们。

暂不允许用一个共享 ERPNext token 代表多个平台用户。如果演示环境只能提供共享账号，可以用 `shared_demo` 连接验证 MCP 链路和合成数据，但用户仍需有明确绑定，且必须显式开启 `ERP_SHARED_DEMO_MODE=true`。该模式标记为“共享 ERP 身份演示”，不能作为双边个人权限互通验收。当前 `bound_user` 使用受限环境 provider 取得绑定用户 token，但还没有两个不同 ERPNext 用户的真实 Role/User Permission 交集验收。

### MVP 的 EBAC 策略

首版只需要直接对平台用户授权，不先引入平台角色同步。策略至少包含：`tenant_id`、`subject_user_id`、`domain`、`tool_id`、`action`、`resource_type`、`resource_scope`、可选 `business_task_id`、`effect`、`policy_revision`、`active`。MVP 对 ERP 只注册明确列出的只读工具；没有匹配的 allow 即拒绝。显式 deny 优先于 allow，所有查询都带 `tenant_id`，任务范围只能缩小资源范围。

`resource_scope` 应是经服务端校验的结构化范围，而不是模型任意提交的过滤表达式；首批只需覆盖客户、物料、仓库和指定 ERP 单据。实现时可将一条策略存为 JSONB，也可按资源拆行，但必须有固定 schema、版本和审计。现有 `integration_grants` 的 `scope` 字符串按其已配置业务含义迁移为资源范围，只允许兼容现有 `list_erp_orders` / `read_erp_order`；绝不把旧 grant 解释为未来新增工具或写入权限。

审批不是 EBAC allow 的同义词：EBAC 决定是否允许调用工具；若以后是写工具，还要另外校验 proposal 版本、审批人/审批状态和 ERP 单据当前状态。

## 4. MCP 服务设计

ERP MCP 是受控的业务工具网关，不是把 Frappe REST API 原样暴露给模型。工具使用稳定的业务 schema，内部适配器再映射到 ERPNext DocType 和 REST 请求。

### 连接和认证

1. Agent Runtime 为一次业务任务运行建立一个 stdio MCP 连接；同一运行的多次工具调用复用它。
2. Runtime Bridge 向网关传递一次性或短期的 Agent session capability。它只证明“哪个任务的哪个运行正在调用”，不直接代表 ERPNext 用户。
3. 网关把调用转发到 Rust API。API 校验 session、租户、平台用户、业务任务、运行状态和 EBAC。
4. API 的 ERP 适配器根据映射取得后台凭据，并以对应 ERPNext 用户调用 ERPNext。凭据不下发给模型或 MCP 子进程。
5. 适配器记录平台判定、ERP 身份、ERPNext 响应和最终业务结果；拒绝时区分平台拒绝与 ERPNext 拒绝，但不泄露未授权记录的存在性。

MCP session 的生命周期与 Runtime/运行绑定，不按每次工具调用创建，也不作为权限缓存。权限和凭据变化后，下一次 API 调用重新检查；撤销或运行结束时，短期 capability 立即失效。

### 工具分层

候选工具目录见[ERP MCP 工具目录](../plantree/plans/server-demo/erp-mcp-tools.md)。下面的分层是权限和生命周期边界，具体工具 ID 可以按目标 ERPNext 版本的 DocType 能力调整。

首批工具保持少量、明确和有界：

| 层 | 示例 | 规则 |
| --- | --- | --- |
| 只读业务查询 | `erp_list_sales_orders`、`erp_read_sales_order`、`erp_get_supply_snapshot` | 固定允许的 DocType；限制页大小、字段、过滤条件和关联范围 |
| 任务关联 | `erp_link_record`、`erp_read_task_links` | 只能关联当前平台业务任务；不改变 ERPNext 业务状态 |
| 变更准备 | `erp_prepare_change` | 生成平台方案和数据快照，不写 ERPNext |
| 执行写入 | `erp_execute_approved_change` | 仅接受后台保存的提案 ID；重新检查 ERPNext 权限、状态、版本和幂等键 |
| 执行跟踪 | `erp_read_execution_status` | 读取真实 ERPNext 单据和执行状态，不能由 Agent todo 代替 |

禁止首版暴露任意 `doctype`、任意 Frappe method、任意 URL、任意脚本执行或“用管理员身份代做”。读写工具分开注册；没有写入验收前，写工具不注册给业务 Agent。

### 调用上下文

网关收到的调用上下文由后台生成，模型不能修改：

```json
{
  "tenant_id": "...",
  "platform_user_id": "...",
  "business_task_id": "...",
  "agent_session_id": "...",
  "agent_run_id": "...",
  "erp_connection_id": "...",
  "erp_user_id": "...",
  "policy_revision": 12
}
```

工具参数只表达业务意图，例如订单号或查询条件；租户、用户、任务和 ERP 身份由后台注入。工具结果要带来源 DocType、记录名、ERP 修改时间或版本摘要以及审计 trace id，供 Agent 和 Web 展示。

## 5. OpenViking 的对应关系

OpenViking 没有 ERPNext 的角色体系，因此其文档访问主要由平台 EBAC、资料元数据和后端范围过滤负责。它仍然走同一类链路：业务任务和平台用户确定客户、项目、物料、版本、有效期范围；MCP 网关只把已过滤的搜索和原文读取交给 OpenViking。

ERPNext 与 OpenViking 可以共享平台身份和任务上下文，但不能共享对方的权限判断。ERPNext 是 ERP 单据权限权威；资料权限服务和索引元数据是知识资料权限权威。

## 6. 审计和错误

每次调用至少记录：平台用户、租户、业务任务、Agent session/run、工具 ID、参数摘要、EBAC 判定、ERPNext 用户、ERP 请求追踪 ID、结果状态和耗时。不得记录 API secret 或完整敏感单据内容。

错误分三类：

- `platform_forbidden`：平台 EBAC 或业务任务范围不允许。
- `erp_forbidden`：映射的 ERPNext 用户被 ERPNext 权限拒绝。
- `erp_state_conflict`：权限允许，但单据版本、状态或前置条件已变化。

面向 Agent 的错误响应在保留 `error` 和 `message` 的同时，对权限和身份配置错误增加 `agent_guidance`：

```json
{
  "error": "erp_forbidden",
  "message": "ERPNext rejected the bound user's permission",
  "agent_guidance": {
    "category": "authorization",
    "retryable": false,
    "safe_next_step": "ask_erp_admin",
    "instruction": "ERPNext denied the bound user. Do not retry with altered arguments or infer hidden records. Ask an ERPNext administrator to grant the required Role/User Permission or bind an authorized ERPNext user."
  }
}
```

`erp_forbidden`、`platform_forbidden` 和范围拒绝都明确不可通过改参数重试；身份未绑定、停用或凭据不可用时，Agent 应联系平台管理员，不能向最终用户索要 ERPNext secret。MCP 网关原样转发这份结构化响应，工具描述也声明了该处理约定。

写入失败或连接断开时，先查询 ERPNext 状态再决定是否重试。Cube 快照、MCP 重连和 Agent 重试都不能证明外部 ERP 写入没有发生。

## 7. 实施顺序和验收

1. 为 `integration_grants` 固定兼容迁移：保留既有 tenant/user/domain/scope/case 限制，同时增加工具/动作、资源类型、effect、策略版本和有效状态；旧行迁移期间默认只赋予明确列出的只读工具，不隐式开放写入。（已完成。）
2. 增加 ERP 连接、平台用户到 ERPNext 用户的唯一有效绑定和后台凭据引用；当前已完成表结构、绑定解析和本地凭据 provider，仍需用两个非管理员 ERPNext 用户验证不同 Role/User Permission 下的结果差异。
3. API 每次调用从已授权的运行上下文派生用户、租户、任务、ERP 连接和策略版本；MCP stdio 只持有短期 session capability，ERP token 只由 API 后台凭据提供器读取。
4. 将现有 `ERP_API_TOKEN` 限制为仅限本地共享身份联调的显式兼容模式；双边权限验收和默认配置必须走绑定用户凭据，生产配置禁止共享/管理员凭据。当前本地绑定 provider 使用 `ERP_CREDENTIAL_*`，生产需替换为 Vault/KMS。
5. 先验收两平台用户、两个客户范围、两个业务任务和两个 ERPNext 权限集下的只读交集，再验收撤销、凭据过期、跨任务/租户、ERPNext 拒绝、MCP 重连和审计。
6. 身份和只读权限验收通过后才加入更多业务推演工具；最后才设计写入工具，并验证提案审批、ERP 状态重检、幂等和断线恢复。

最低验收矩阵：

| 平台 EBAC | ERPNext 权限 | 结果 |
| --- | --- | --- |
| 允许 | 允许 | 调用成功，结果仅在两边范围交集内 |
| 允许 | 拒绝 | `erp_forbidden`，不返回未授权记录详情 |
| 拒绝 | 允许 | `platform_forbidden`，不发出 ERP 请求 |
| 任务不关联记录 | 允许 | 仅允许明确的全局只读工具；任务限定工具拒绝 |
| 已批准但 ERP 状态变化 | 允许 | `erp_state_conflict`，不直接重放写入 |

## 8. 当前不做的事

- 不把业务任务同步成 ERPNext 的“案件”或新增财务模块。
- 不在平台复制 ERPNext 的完整角色管理页面。
- 不让 Agent 直接拿 ERPNext API key。
- 不用 MCP session ID 作为用户身份或长期授权凭据。
- 不把平台 EBAC 的“允许调用工具”解释为 ERPNext 的“允许修改单据”。
