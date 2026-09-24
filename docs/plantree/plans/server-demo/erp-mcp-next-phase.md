# ERP MCP 下一阶段设计

状态：本地双用户只读交集已验收，进入 Phase A 正式凭据与 capability 生命周期；当前结果仍不是生产部署承诺。本文件定义从只读 MVP 走向可扩展、可审计、可受控写入的实施顺序。

## 目标和边界

目标是让 Agent 能够在一个业务任务中安全地完成“读取事实、解释影响、形成方案、等待审批、执行一次、核对结果”的闭环。ERPNext 仍然是 ERP 单据权限和状态的权威，平台 EBAC 仍然只负责平台用户能否发起某个业务动作以及任务/资源范围。

本阶段不做以下事情：

- 不把任意 Frappe REST、DocType、脚本或 SQL 暴露给模型。
- 不把平台角色同步成 ERPNext Role，也不在平台复制 ERPNext 权限页面。
- 不让 Agent 直接接触 ERPNext token、Vault token 或连接密钥。
- 不把“Agent todo 已完成”当作 ERP 执行完成。
- 不在真实双边权限、审批、状态重检和幂等验收前注册写入工具。

## 总体分层

```text
JCode Agent
  -> stdio ERP MCP gateway
  -> short-lived session capability
  -> Rust API: identity + EBAC + task scope + approval + audit
  -> typed ERP domain adapter
  -> credential provider / Vault-KMS
  -> ERPNext user API
```

MCP session 只表示一次 Runtime 连接和调用上下文。每次工具调用仍由 API 重新检查平台用户、租户、业务任务、运行状态、工具授权和 ERP 用户绑定；session 不缓存长期权限，也不作为 ERPNext 身份。

### Phase A 本地实证

在真实本地 ERPNext v16.35.0 HTTP API 上完成了两个非管理员映射用户的只读交集验证：`forge-reader@example.test` 在平台案件范围内读到 Amazon `SAL-ORD-2026-00003`；`limited-reader@example.test` 仅读到其 ERPNext User Permission 允许的 Temu `SAL-ORD-2026-00001`。受限用户读取 Shopee `SAL-ORD-2026-00002` 时由 ERPNext 原生返回 403，网关返回 `erp_forbidden`、`retryable=false`、`safe_next_step=ask_erp_admin`，没有泄露未授权单据详情。平台 deny 返回 `platform_forbidden`，案件客户越出 grant 返回 `erp_scope_missing`，且均有审计 trace。验证脚本为 `scripts/validate-real-erp-boundary.mjs`；只使用合成用户和订单，未执行 ERP 写入。

## 三阶段路线

### Phase A：只读生产化

先把已有两个工具扩展成稳定的业务工具契约，并完成真实双边权限验收。

- 凭据 provider 从本地 `ERP_CREDENTIAL_*` 抽象为 Vault/KMS 适配器，支持版本、过期、撤销、轮换和最短缓存；密钥只在 Rust API 进程内短暂可见。
- 固定 session capability 字段：`tenant_id`、`platform_user_id`、`business_task_id`、`agent_session_id`、`agent_run_id`、`expires_at`、`policy_revision` 和随机 nonce。
- 所有 ERP 读取工具使用统一结果信封：`status`、`as_of`、`trace_id`、`sources`、`data`、`warnings`。来源至少带 DocType、记录名和 ERP `modified` 时间。
- 建立 `erp_forbidden`、`platform_forbidden`、`erp_scope_missing`、`not_found`、`erp_unavailable`、`unsupported_capability`、`stale_result` 的稳定错误分类；权限错误不泄露记录存在性。
- 已用两个非管理员 ERPNext 用户、两个客户范围和两个业务任务完成本地验证：平台允许/ERP 允许、平台允许/ERP 拒绝、平台拒绝/ERP 允许和跨任务范围；撤销/过期仍待正式凭据 provider 验证。

Phase A 的交付门槛是：读取结果能回到具体 ERP 记录和审计 trace；ERPNext 拒绝时不返回敏感单据详情；MCP 重连或模型重复调用不会放大权限。

### Phase B：只读业务推演

在 Phase A 通过后，按固定 DocType 和字段边界增加业务工具。推演只计算，不改变 ERPNext 状态。

首批工具：

| 工具 | 用途 | 依赖 |
| --- | --- | --- |
| `erp_production_readiness_check` | 检查订单能否按期生产 | Sales Order、BOM、库存、开放采购/工单 |
| `erp_shortage_explainer` | 解释物料缺口 | Bin、预留、在途和需求来源 |
| `erp_order_change_impact` | 模拟数量/日期变化的影响 | 订单、BOM、采购、工单 |
| `erp_production_plan_preview` | 预览净需求和计划缺口 | BOM 展开、库存和仓库范围 |
| `erp_delivery_risk_radar` | 返回风险带和证据 | 交期、工单、采购和库存的新鲜度 |

每个工具只接受业务参数，不接受 `tenant_id`、ERP 用户、DocType、字段列表、任意过滤表达式或 Frappe URL。缺少目标 ERPNext 版本字段时返回 `unsupported_capability` 或 `unknown`，不自行选择替代口径。

推演结果必须带 `simulation_id`、输入快照摘要、来源版本和 `as_of`。Agent 只能解释推演，不能把推演当作 ERP 已执行。

### Phase C：受控写入

写入从“方案”开始，不从模型自由参数开始。建议状态机：

```text
draft
  -> awaiting_approval
  -> approved
  -> executing
  -> applied | rejected | unknown | failed
```

执行工具只接受后台生成的 `proposal_id` 和服务端生成的 `idempotency_key`。执行前必须在同一个后端用例中重新检查：

1. proposal 版本、动作列表摘要和批准状态；
2. 当前平台 EBAC、业务任务范围和 ERPNext 用户绑定；
3. ERPNext 单据状态、权限、版本/`modified` 和必要前置数据；
4. 幂等键是否已成功执行、正在执行或结果未知。

执行后必须查询 ERPNext 验证真实单据状态，再写入平台执行记录。连接在写入后断开时标记 `unknown`，先对账再决定是否可恢复，不能直接重放。

首批只考虑“创建草稿/申请”类动作，例如 `Material Request` 草稿、`Production Plan` 草稿和 `Work Order` 草稿；不直接自动提交、审核或删除。每个写工具单独注册、单独授权、单独审批。

## 工具契约

所有工具采用同一调用上下文，后台注入而非模型传入：

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

所有读取和推演结果使用：

```json
{
  "status": "ok",
  "as_of": "2026-09-24T08:30:00Z",
  "trace_id": "...",
  "sources": [
    {"doctype": "Sales Order", "name": "SO-...", "modified": "..."}
  ],
  "data": {},
  "warnings": []
}
```

结果只返回完成业务问题所需的字段，不透传 ERPNext 原始响应；字段权限、客户范围、任务范围和敏感字段脱敏都在 API/适配器完成。

## 权限和审计

一次调用的有效权限为：

```text
平台 EBAC
  ∩ 业务任务资源范围
  ∩ ERPNext 映射用户的 Role/User Permission/字段权限
  ∩ 工具固定的 DocType/字段/关联过滤
```

显式 deny 优先，旧 grant 不能授权新工具或写动作。每次调用审计：平台用户、租户、任务、session/run、工具 ID、参数摘要、policy revision、EBAC 判定、ERP 用户、ERP trace、结果状态、耗时和错误分类；不记录 secret 和完整单据内容。

## 验收矩阵

| 场景 | 预期 |
| --- | --- |
| 平台允许，ERPNext 允许 | 只返回两边交集内的数据，并带来源和 trace |
| 平台允许，ERPNext 拒绝 | `erp_forbidden`，`retryable=false`，不返回未授权详情 |
| 平台拒绝，ERPNext 允许 | `platform_forbidden`，不发出 ERP 请求 |
| 任务范围不含记录 | `erp_scope_missing`，不允许 Agent 自行扩大范围 |
| 绑定撤销/凭据过期 | 调用失败并要求管理员修复，不向用户索要 secret |
| 推演输入版本变化 | `stale_result` 或 `erp_state_conflict`，不能当作当前事实 |
| 写入后连接断开 | `unknown`，先查询核对，不直接重放 |
| 相同 proposal/idempotency key 重复调用 | 不产生重复 ERP 单据 |
| MCP 重连或 Agent 重试 | 重新鉴权，不继承已撤销的权限 |

## 实施顺序

1. 固定统一结果/错误 envelope 和 capability 生命周期，补充 OpenAPI/JSON Schema 与 API 单测。
2. 接入正式后台凭据 provider，做轮换、撤销、过期和审计验证。
3. 已在本地 ERPNext v16.35.0 上完成第二个受限用户和 Role/User Permission 双边验收；下一步改为在目标环境复核版本字段、正式凭据 provider、撤销/过期轮换和生产审计。
4. 实现 Phase B 的四个只读推演工具，逐个固定字段、数据新鲜度和未知结果规则。
5. 增加 proposal/execution/reconciliation 持久化模型，但先只生成方案，不注册写入 MCP 工具。
6. 选择一个草稿类写工具做端到端验收，完成审批、状态重检、幂等、断线和对账后再扩展其他写工具。

## 当前开放问题

- 目标 ERPNext 的确切版本、启用的制造模块和实际字段是否覆盖 Phase B 所需的库存预留、交期和工序日历。
- 正式凭据服务选 Vault、云 KMS 封装还是现有平台 secret service，以及轮换期间的旧版本保留时间。
- 首个写入动作选择 Material Request 草稿、Production Plan 草稿还是 Work Order 草稿。
- 审批人是否必须与 ERPNext 用户不同，以及 proposal 是否需要双人审批。
- 目标业务的“可用库存”口径是现有量、现有减预留，还是包含已确认在途。
