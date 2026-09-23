# 流式中断回归与修复

日期：2026-09-22。本机联调，不代表生产验收。

## 原因

- 旧接入代码为 JCode 回合硬编码 150 秒、子进程 165 秒总时限；这是本项目设置，不是 Cube 或模型要求。运行记录 59 在约 150.3 秒失败。
- API 的展示记录保存了部分正文，但 JCode 在完整流结束前未保存 Assistant 消息。超时丢弃 future、提供方断流或强杀会造成下一轮模型历史缺失已显示内容。
- 修复时先替换了 Runtime 二进制，正在运行的旧 API 不接受新增 `stream_checkpoint` 文档，返回 HTTP 400，导致问候也失败（记录 62–69）。更新顺序是开发操作失误。

## 已修改

- 总时限默认关闭，`AGENT_TURN_TIMEOUT_SECONDS=0`。正数为显式选配，范围 1–3600 秒；网络请求、资源大小、停止后的退出宽限仍各自有界。
- 新增迁移 017 和 PostgreSQL 流式检查点。在发布正文前保存精确消息锚点及正文；正常追加 Assistant 后清空。新进程按锚点恢复，重复恢复不重复追加。没有 JSON/JSONL 会话副本。
- 提供方错误和不完整 EOF 先保存已有 Assistant 内容，再返回失败；停止和 API 退出先发 SIGTERM，给原生 Loop 保存机会。API 在 Worker 退出前维持存储 HTTP 接口与租约。
- 页面保留已生成内容；超时、停止和异常有不同状态。历史失败记录不伪装成成功，也不自动重新执行旧工具。

## 证据

- `npm run check`：类型检查、Rust API/MCP 构建、13 组原有 PostgreSQL 集成检查和 Web 构建通过。
- `node --env-file=.env scripts/test-stream-recovery.mjs`：原生 JCode + 随机隔离 PostgreSQL 数据库，normal、stream_error、event_error、EOF、stop、模拟 timeout、SIGKILL 七组通过。每组重启 API、换新进程，让下一次 Provider 调用断言收到历史正文，检查只追加一次。
- `VERIFY_LONG_TURN=1 node --env-file=.env tests/system/test-rust-api.mjs`：新增 API 退出期间写检查点测试、151 秒 Worker 任务均通过。长任务使用受控 Runtime fixture，不是模型性能数据。
- `scripts/verify-live-stream-recovery.mjs`：真实 DeepSeek Pro 高思考输出后主动停止，274 字符正文与 JCode PostgreSQL 消息完全一致；随后新进程的 Flash 回合读回第一行标记。浏览器刷新后桌面/移动端无页面错误或横向溢出。证据 `/tmp/erp-live-stream-recovery/report.json`、`recovered-{1440,390}.png`。
- 真实问候及新对话在 API/Runtime/迁移同步后完成；`/ready` 返回 200。

## 批准与输入框

请求批准、帮我批准、完全访问现已在 Rust 后端实现。受控操作包括个人 `memory_save`、案件 `case_set_context` 和提案 `proposal_create`；ERP/知识读取在三种模式下可用，业务提案审批和 ERP 写入仍为独立边界。当前回合的权限上下文注入 JCode 系统提示词，`case_read` 也返回实时授权对象。批准绑定原始工具调用 ID 和参数摘要，拒绝不会写入，代为批准使用独立模型请求并在解析失败时拒绝。

`scripts/verify-tool-approvals.mjs` 真实验证 ask/允许、ask/拒绝、auto/允许、full/允许，原始工具调用及结果各一次，用户输入只一条，个人记忆效果正确；1440/390/320 批准卡片刷新保留。证据 `/tmp/erp-tool-approvals/report.json`。独立审核拒绝/无效响应、跨用户、参数篡改、重复决定、过期、停止、重启由 PostgreSQL fixture 集成测试覆盖。

灰白模型列表、思考程度滑块、端点展示及输入框向上增高/达到上限滚动已完成浏览器检查，证据 `/tmp/erp-approval-ui/report.json`。

## 边界与更新顺序

以上不代表所有故障已穷尽。旧版已丢失的模型历史未自动修补，原展示文本仍保留。检查点当前每个正文块一次读写，尚未完成多用户吞吐测量；批量写入优化必须保持先提交再显示。OpenViking 实际检索和 Cube 内 Harness 仍未验收。

后续先在独立数据库/端口验证配套 API、迁移和 Runtime；先更新兼容存储 API，再启用新 Runtime。正在服务的二进制应使用固定副本，不能指向会被随手 `cargo build` 覆盖的开发产物。
