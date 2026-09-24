# ERP MCP 与 EBAC

状态：In Progress。工具级 EBAC、ERP 连接、平台用户到 ERPNext 用户绑定和本地个人凭据提供器已实现；ERPNext 原生权限交集和正式密钥服务仍未完成。

已落地的第一步：`integration_grants` 增加 `tool_id`、`action`、`resource_type`、`effect`、`policy_revision` 和 `active`；Rust MCP 调用入口先按具体工具执行 allow/deny 判定，再把允许的 scope 交给 ERP/OpenViking 适配器。旧的无 `tool_id` grant 只兼容当前已发布的四个只读工具，不能授权未来工具或写入动作；显式 deny 优先。

当前已增加 `erp_connections` 和 `erp_identity_bindings`。演示初始化会把平台用户绑定到明确的 `forge-reader@example.test`，以 `bound_user` 连接读取后台环境中的 `ERP_CREDENTIAL_FORGE_READER`；未绑定用户即使开启演示模式也不能调用 ERP。审计只记录 ERP 用户标识，不记录密钥。凭据引用仅允许 `ERP_CREDENTIAL_*` 环境变量，生产环境仍应替换为 Vault/KMS provider。Rust 单测和 `npm run check` 已通过。

ERP 权限拒绝已经有 Agent-facing 错误契约：API 在 `erp_forbidden`、`platform_forbidden`、范围拒绝、身份绑定和凭据配置错误中附加 `agent_guidance`，说明错误类别、是否可重试、安全的下一步和给 Agent 的处理指令；MCP 网关直接转发这份响应。权限拒绝不会提示 Agent 通过改变参数绕过，也不会让 Agent 向用户索要 ERP secret。

## 目标

建立平台业务任务、平台 EBAC、ERPNext 原生用户权限和 ERP MCP 网关之间的可执行边界。业务任务是 Agent 的工作上下文，不是 ERPNext 的案件或权限模型。

## 已确认方向

- ERPNext 保留用户、角色、Role Permission、User Permission、字段权限和单据状态作为 ERP 权限来源。
- 平台 EBAC 只负责平台用户能否发起某个 Agent 工具/动作，以及任务和资源范围。
- 一次调用同时经过平台 EBAC 和 ERPNext，结果取两边允许范围的交集。
- ERP MCP 暴露业务工具，不暴露任意 Frappe REST/DocType/脚本能力。
- MCP session 是 Runtime 连接上下文，不是登录身份，也不替代权限检查。
- 当前 `cases` 表先兼容保留，产品语义改为业务任务/变更事项；ERPNext 单据通过引用关联。

## 推荐的 MVP 身份方案

- 平台用户由管理员显式绑定一个 ERPNext 用户；ERPNext 账号仍在 ERPNext 创建和停用，平台不做自注册/自动 provision。
- 绑定凭据引用 ERPNext 该用户自己的 API 凭据。API 负责取密钥并直接以该用户访问 ERPNext；MCP 子进程和模型看不到密钥。
- 暂不双向同步角色，也不把平台角色映射成 ERPNext 角色。EBAC 先授权工具、动作、业务任务和客户/物料等资源范围，最终记录权限由 ERPNext 自己判定。
- MVP 不接受共享 ERP token 作为个人权限互通方案。现有进程级 `ERP_API_TOKEN` 只在 `shared_demo` 连接且显式开启 `ERP_SHARED_DEMO_MODE=true` 时用于本地链路联调；正式演示走绑定用户自己的 `ERP_CREDENTIAL_*` 引用。
- `integration_grants` 已有工具、动作、资源类型和效果字段；旧的 domain/scope/case grant 只兼容已发布的只读工具，不会自动继承未来写动作。

## 设计文件

- [ERPNext MCP 与 EBAC 设计](../../../architecture/erp-mcp-ebac-design.md)
- [ERP MCP 工具目录](erp-mcp-tools.md)
- [已有审批与工具权限](approval-workflow.md)
- [会话存储边界](../../../architecture/session-storage.md)

## 待完善

1. EBAC 的最小策略字段：主体、工具、动作、资源类型、资源范围、效果和版本。（工具级字段已落地，结构化资源范围仍待实现。）
2. 平台用户与 ERPNext 用户按租户和 ERP 连接一对一绑定；`erp_connections` 与 `erp_identity_bindings` 已落地，绑定用户凭据读取已支持本地受限 provider。
3. ERP 凭据存储位置和轮换方式；不得进入浏览器、模型或 Cube 镜像。当前 provider 只为本地联调读取 `ERP_CREDENTIAL_*`，生产需接入 Vault/KMS；`shared_demo` 仅允许显式本地联调配置。
4. 业务任务关联 ERPNext 单据的字段、快照摘要和版本表示。
5. 读取工具的首批 DocType/字段/过滤边界，以及写工具暂不注册的范围。
6. 绑定唯一约束、凭据轮换/撤销语义，以及两个真实 `bound_user` 身份的 ERPNext 原生权限交集验收。

## 验收门槛

- 能证明平台允许但 ERPNext 拒绝时不会发回单据详情。
- 能证明平台拒绝时不会发出 ERPNext 请求。
- 能证明跨租户、跨任务和撤销后的 MCP 重连均被拒绝。
- 能证明 ERPNext 用户、平台用户、任务、工具调用和审计 trace 可以互相追溯。
- 写入工具只有在提案版本、审批、ERP 状态重检和幂等测试通过后才开放。
