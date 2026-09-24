# ERP MCP 工具目录

状态：Planning。以下是 ERP MCP 的候选工具目录，不代表当前网关已经发布这些工具。当前实现仍只有两个 ERP 只读工具：`list_erp_orders` 和 `read_erp_order`。

## 落地顺序

候选目录用于定义产品能力，不意味着首轮全部实现。建议按可验证的数据依赖分批：

1. **只读试点**：`erp_production_readiness_check`、`erp_shortage_explainer`、`erp_order_change_impact`、`erp_production_plan_preview`。先固定订单、BOM、库存和未结采购/工单的字段、权限过滤与版本摘要。
2. **风险与追踪**：`erp_delivery_risk_radar`、`erp_trace_order_supply_chain`、`erp_data_freshness_check`。依赖首批关联查询准确后再加入，缺少排程或交期证据时明确返回 `unknown`。
3. **有限推演**：`erp_inventory_what_if`、`erp_find_item_substitutes`、`erp_compare_bom_revisions`、`erp_bottleneck_radar`、`erp_supplier_shortlist`。仅使用 ERPNext 中已配置且当前用户可见的数据，保留计算输入和版本。
4. **平台任务整合与写入**：接入任务证据和 proposal；ERPNext 写工具暂不发布，直到身份映射、审批、状态重检、幂等和重复/断线恢复测试通过。

第一批是建议顺序，不代表已经实现或完成 ERPNext 版本兼容验证。开始实现前要检查部署版本、启用模块和实际字段；报表或对象名称不能替代服务端权限校验。

## 设计规则

- 工具解决一个可描述的生产运营问题，不暴露任意 Frappe URL、DocType、脚本或 SQL。
- 所有结果带 `as_of`、来源单据、记录修改时间或版本摘要、授权范围和 `trace_id`。
- 计算和推演在后台完成，模型只解释结果；模型不能把推演结果当成 ERP 已写入。
- 首批工具全部只读或只写平台方案。创建 ERPNext 单据的工具必须单独审批、重检状态并使用幂等键。
- 参数中的订单、客户、物料和仓库只能在后台注入或校验，不能用参数绕过 EBAC 和 ERPNext 权限。
- 工具 ID 稳定，界面显示友好的动作名称，例如“检查能否按期生产”“模拟数量变化”“寻找替代料”。

## 首批调用契约草案

MCP schema 只接受下列业务参数。`tenant_id`、平台用户、业务任务、ERP 连接、ERPNext 用户和授权范围由 API 根据已认证的 Agent run 派生；模型不能在参数中指定这些字段，也不能传任意 DocType、字段列表或 Frappe URL。

| Tool ID | 最小参数 | 必要授权 | 固定读取范围 |
| --- | --- | --- | --- |
| `erp_production_readiness_check` | `sales_order` | `erp.sales_order.readiness.read` + 该订单所属客户/任务范围 | Sales Order 明细、有效 BOM、库存/预留口径、未关闭采购与工单；缺配置返回未知项 |
| `erp_shortage_explainer` | `item_code`、`warehouse`、`required_qty`、可选 `required_by` | `erp.stock.shortage.read` + 物料和仓库范围 | 库存水平、预留、可见的在途采购/物料申请及需求来源 |
| `erp_order_change_impact` | `sales_order`、可选 `proposed_qty`、可选 `proposed_delivery_date` | 订单读取授权 + `erp.sales_order.change_impact.read` | 只模拟指定行/订单数量或日期变化；不得更新销售订单 |
| `erp_production_plan_preview` | `sales_order`、可选 `warehouse`、可选 `planned_start_date` | 订单读取授权 + `erp.production_plan.preview` | 订单需求、BOM 展开和库存净需求；仅调用读取/计算路径，不创建 Production Plan |

上面的 permission key 是平台 EBAC 的业务动作标识，不要求与 ERPNext Role 名称一致。API 同时用映射的 ERPNext 用户请求数据；ERPNext 拒绝时，不可用平台授权兜底。若 ERPNext 版本/配置缺少短缺计算所需字段，则返回 `unsupported_capability` 或 `unknown`，不猜测替代口径。

建议各工具返回共同信封，业务数据放在 `data`：

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

`as_of` 是本次 API 读取时间，不是 ERP 数据更新时间；每条来源单据需单独携带 `modified`。只返回工具完成任务所需的字段，不透传 ERPNext 原始响应。错误 envelope 至少区分 `platform_forbidden`、`erp_forbidden`、`not_found`、`erp_not_configured`、`unsupported_capability`、`erp_state_conflict`、`stale_result` 和 `erp_unavailable`；`not_found` 对平台无权或 ERPNext 无权的单据保持不可枚举。

## 第一组：侦察工具

这些工具只读取 ERPNext，不改变业务状态。

| Tool ID | 人话名称 | 输入 | 输出重点 | 主要 ERPNext 对象 |
| --- | --- | --- | --- | --- |
| `erp_production_readiness_check` | 检查能否按期生产 | 销售订单、目标数量、目标日期 | 成品库存、BOM 展开、缺料、开放工单、待采购、明确阻塞项 | Sales Order、BOM、Bin、Work Order、Purchase Order |
| `erp_delivery_risk_radar` | 看交付风险 | 销售订单或订单集合 | `low/medium/high/unknown` 风险带、原因和证据，不输出伪精确概率 | Sales Order、Work Order、Stock、Purchase Order |
| `erp_order_change_impact` | 看变更会影响什么 | 订单、旧数量/日期、新数量/日期 | 受影响的物料、采购、生产、交付和待确认项 | Sales Order、BOM、Material Request、Work Order |
| `erp_trace_order_supply_chain` | 沿订单追物料链 | 销售订单、最大深度 | 有界的订单到物料、库存、采购、生产关联图 | Sales Order、BOM、Bin、Purchase Order、Work Order |
| `erp_shortage_explainer` | 解释为什么缺料 | 物料、数量、仓库、需求日期 | 可用量、预留量、在途、开放采购、上游需求和短缺原因 | Bin、Stock Ledger、Purchase Order、Material Request |
| `erp_data_freshness_check` | 检查数据新鲜度 | 一组已关联的 ERP 记录 | 各记录最后修改时间、查询时间差、可能过期的结果 | 传入记录涉及的 DocType |

## 第二组：推演工具

这些工具可以做有界计算，但不创建 ERPNext 单据。

| Tool ID | 人话名称 | 输入 | 输出重点 | 备注 |
| --- | --- | --- | --- | --- |
| `erp_inventory_what_if` | 模拟库存变化 | 物料、仓库、数量增减、需求日期 | 模拟后的可用量、净缺口和受影响订单 | 结果带 `simulation_id`，不写 Bin |
| `erp_find_item_substitutes` | 寻找替代料 | 物料、数量、仓库、目标日期 | 替代料候选、可用量、BOM 关系、单位换算和人工确认项 | 替代关系以目标 ERPNext 配置为准 |
| `erp_compare_bom_revisions` | 比较 BOM 版本 | 两个 BOM 或物料+版本 | 新增/删除/数量变化、受影响工序和资料引用 | 不假设版本号天然代表有效期 |
| `erp_bottleneck_radar` | 找产能瓶颈 | 物料/工单集合、时间窗口 | 工作站或工序负载、冲突工单、缺少日历数据的警告 | 没有排程约束时返回 `unknown` |
| `erp_supplier_shortlist` | 找供应商候选 | 物料、数量、目标日期 | 已配置供应商、历史交期、开放采购和价格来源 | 只提供候选，不自动选定供应商 |
| `erp_production_plan_preview` | 预览生产计划 | 订单、数量、仓库、日期 | BOM 展开后的成品/原料需求、计划缺口和建议动作 | 只返回预览，不创建 Production Plan |

## 第三组：任务和证据工具

这些工具把 ERP 事实放入平台业务任务，但不修改 ERPNext。

| Tool ID | 人话名称 | 输入 | 输出重点 |
| --- | --- | --- | --- |
| `erp_link_task_record` | 关联 ERP 单据 | 业务任务、DocType、记录名 | 记录引用、权限检查结果和观察版本 |
| `erp_read_task_records` | 查看任务关联单据 | 业务任务、过滤条件 | 当前可访问的关联单据和新鲜度 |
| `erp_prepare_change_proposal` | 整理变更方案 | 推演结果、动作列表、依据 | 平台 proposal、摘要、数据版本和待审批动作；不写 ERPNext |
| `erp_read_execution_timeline` | 查看执行时间线 | 业务任务或 proposal | ERP 单据状态变化、平台审计和最后核验时间 |

## 第四组：后续受控写工具

这些工具先只设计，不注册给当前业务 Agent。它们必须接受后台 proposal ID，而不是接受模型传入的“已批准”布尔值。

| Tool ID | 人话名称 | 可能写入 | 额外门槛 |
| --- | --- | --- | --- |
| `erp_create_material_request_from_proposal` | 创建物料申请草稿 | Material Request | 方案审批、ERP 用户权限、物料状态、重复键 |
| `erp_create_production_plan_from_proposal` | 创建生产计划草稿 | Production Plan | 方案审批、BOM/库存重检、日期和数量版本 |
| `erp_create_work_order_from_proposal` | 创建工单草稿 | Work Order | 生产计划来源、BOM、工作站和状态重检 |
| `erp_reconcile_approved_execution` | 对账已批准执行 | 平台执行记录，不直接猜测 ERP 状态 | 查询优先、重复执行保护、异常转人工 |

## 好玩的组合示例

### “订单能不能按期交付？”

Agent 先调用 `erp_production_readiness_check`，发现原料不足后调用 `erp_shortage_explainer`，再调用 `erp_supplier_shortlist`。如果用户把数量从 1,000 改成 1,300，Agent 使用 `erp_inventory_what_if` 和 `erp_order_change_impact` 做推演，最后通过 `erp_prepare_change_proposal` 保存方案。

### “这张订单为什么是红色？”

`erp_delivery_risk_radar` 返回风险带和证据；Agent 再按证据调用 `erp_trace_order_supply_chain`，把红色原因拆成“成品不足”“某物料预计到货晚于交期”或“工序日历缺失”，而不是自行编造解释。

### “换一个料会怎样？”

`erp_find_item_substitutes` 找候选，`erp_compare_bom_revisions` 比较 BOM 影响，`erp_inventory_what_if` 计算数量变化；结果只形成待确认方案，不自动修改 BOM 或工单。

## 最小验收集

1. 两个用户看到同一订单时，因 ERPNext Role/User Permission 或 EBAC 不同而得到不同结果。
2. 推演工具不会改变 ERPNext 的库存、BOM、订单、采购或生产单据。
3. 过期或关联版本变化时，工具返回 `erp_state_conflict` 或 `stale_result`，不让 Agent 当作当前事实。
4. 每个结果可以回到具体 ERPNext DocType、记录名、修改时间和 trace。
5. MCP 重连、重复调用和模型重试不会重复创建任何 ERP 单据；写工具在未通过审批和幂等验收前不发布。

## ERPNext 参考映射

以下官方文档用于确认业务对象和标准流程；目标站点的 ERPNext 版本、字段、已安装应用和权限配置仍需现场核验。

- [Production Plan](https://docs.frappe.io/erpnext/production-plan)：生产计划可围绕销售需求和物料短缺形成计划；对应本目录的计划预览和未来受控计划草稿。
- [Work Order](https://docs.frappe.io/erpnext/work-order)：生产工单对象；对应就绪检查、追踪和未来受控工单草稿。
- [Stock Level Report](https://docs.frappe.io/erpnext/stock-level-report)：库存水平视图；可作为库存/短缺解释的核对入口，具体可用量仍需定义仓库、预留和在途口径。
- [Production Planning Report](https://docs.frappe.io/erpnext/production-planning-report)：生产规划报表；用于核对订单、产品、库存与 BOM 相关的计划视图。
