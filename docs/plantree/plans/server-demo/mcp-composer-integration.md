# MCP 与会话输入联调

日期：2026-09-22。本机联调，不代表生产部署或 ERP 写入验收。

## 已观察结果

- Rust MCP 网关通过 JCode 原生 McpManager 注册 `erp`、`openviking` 两个 stdio 服务，合计四个领域工具。业务身份、用户/案件范围和服务凭据由 Rust API 检查与持有。
- 真实模型依次完成 ERPNext 订单列表和订单明细读取；OpenViking 工具被实际调用，返回 `openviking_not_configured`。模型明确说明未检索知识库，未生成虚假证据。单次完整运行 10,121 ms，只是该次观察，不是性能基准。
- ERPNext 使用专用只读服务账号，用户范围通过 PostgreSQL integration_grants 限定。当前演示账号有四个合成客户授权；新注册用户不会自动获得这些数据。
- OpenViking 接口及根 URI 范围验证已实现，并有隔离 fixture 检查；真实服务、Embedding 和文档索引尚未配置。用户已确认 Embedding 可以随后配置。
- 修复 DeepSeek Responses 连续工具调用中历史 reasoning item 顺序导致的 400；保留推理数据并放在对应 assistant 消息的正文/工具调用之前。真实列表、读取和检索工具循环通过，provider 26 项单元测试通过。
- 新建会话直接进入空聊天，业务字段可稍后补充。模型端点、Flash/Pro、思考程度选择持久化到会话/消息/运行记录；恢复已有 JCode 会话后重新应用所选模型与思考程度。真实 Pro 无思考回复及切换 Flash/max 后停止均通过。
- 输入框附件、压缩、模型/思考菜单、语音按钮、发送/停止已接入。语音使用浏览器 SpeechRecognition，未做实体麦克风验收。停止实际终止当前模型子进程并保留部分输出及 cancelled 状态。
- 先前加入的 `chat/read` 菜单不符合用户要求；用户澄清为“请求批准 / 帮我批准 / 完全访问”。三种业务操作模式已由 Rust API、JCode Loop 输入和权限上下文实现；`request_proposal_approval` 仍要求独立的人类审批。

## 验证证据

- `npm run check`：Web 类型检查、Rust/API/MCP 构建、PostgreSQL 隔离集成测试及 Web 构建通过。
- `npm run agent:test`：Skill 边界通过；API 知识 URI 逃逸检查通过。
- `scripts/verify-agent-mcp.mjs`：真实 Responses 与四个领域工具注册验证；报告 `/tmp/erp-mcp-final/report.json`，case `bba00173-68c7-4fb2-bcde-4a46c4245b34`。
- 认证回归十项通过：登录、注册、空会话、刷新、多标签退出、撤销会话、503 恢复和用户隔离。报告 `/tmp/erp-mcp-auth/report.json`。
- `scripts/verify-web-agent-controls.mjs`：真实工具时间线、1440/390/320 输入控件、模型设置恢复、真实停止通过。补充 Markdown 表格渲染，并修复宽表格撑开移动端 Grid 以及 320px 工具栏换行；新增输入框/页头完整位于视口的检查。最终 Web 构建和四组浏览器检查通过，报告及截图 `/tmp/erp-mcp-ui-final/`。

当前服务入口 Web `http://127.0.0.1:4173/`，Rust API `http://127.0.0.1:3100`。Harness 仍在宿主进程；没有 ERP 写工具，没有真实解析/RAG 完整验收。
