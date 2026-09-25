# DeepSeek / JCode 本地联调

日期：2026-09-22。范围：本机合成案件与专用 DeepSeek 模型账号；未调用 ERPNext 或 OpenViking，未写入 ERP，未部署生产。

本文记录首轮联调。后续已增加业务 Skill 发现/加载，并修复多工具场景的原生 reasoning 历史与调用排序；工具范围、测试和重启证据见 [Skill/解析基础联调](skill-parser-integration.md)。最新 Rust API、个人记忆和真实手动压缩/恢复证据见 [Rust/认证/压缩验收](rust-auth-context.md)。

## 已实现链路

Web 新消息经业务 API 鉴权后，与审计和 `agent_runs` 队列记录同事务提交。Worker 使用 PostgreSQL 案件 advisory lock，重新检查用户权限并签发案件绑定的存储令牌，启动 Rust Bridge。Bridge 直接调用固定版本 JCode `Agent`，由 JCode 负责模型、工具结果和后续模型请求的循环。

专用配置存放在忽略提交的 `.runtime/deepseek.env`，目录 0700、文件 0600。默认模型为 `deepseek-flash`，从服务 `/models` 目录确认可用。模型请求使用 DeepSeek 原生 `POST /responses`，无 Chat Completions 回退。每轮提供完整的有界历史和函数输出；关闭不适用的 OpenAI 原生压缩接口。

JCode 只注册 `case_read` 和上游 `todo`，未初始化通用编码工具注册表。`case_read` 返回后台按身份读取的当前案件快照，并非 ERP 订单数据。模型无法扩大案件范围，也不能创建方案、审批或执行 ERP 写入。

专用子进程环境不继承开发者账号；临时 HOME、工作目录和 PID 文件在结束后清理。JCode 消息、工具结果、todo 等通过远程存储接口进入 PostgreSQL，使用版本比较和 fencing。没有 JSON/JSONL 会话副本或后台双写。当前 SDK/server 部分入口仍依赖文件存储，因此采用 Rust Agent 接口，不能把本次验收表述为 SDK/socket 路径完成。

## 思考与打字

DeepSeek 的 `response.reasoning_text.delta` 被映射到 JCode `ThinkingDelta`，再由 Bridge 转发 `reasoning_delta` / `reasoning_done`。API 把思考片段和工具调用记录到有序 `steps`，与回答文本分别保存。运行中断时保留已有内容并结束运行状态；失联任务不会自动重放。

Web 用紧凑、可键盘展开的思考区呈现这些片段，具体工具调用插在相应位置。此组件参考 Astryx ChatReasoning 的结构；并未导入实验性 Lab 组件。正文实际使用稳定版 `@astryxdesign/core@0.6.2` 的 `useStreamingText` 平滑输出，带输入光标、Markdown、中文字符边界保护及减少动态效果支持。新回复完成后继续显示尚未播放完的文字，历史回复刷新后直接显示全文。

传输方式目前为上游 SSE → JCode/Bridge 事件 → PostgreSQL 合批保存 → Web 轮询；浏览器在上一次读取结束 600 ms 后再读取，未实现浏览器 SSE。文字显示是对真实返回内容的平滑处理，未生成演示思考内容。

## 观测证据

- `npm run check`：20 项真实 PostgreSQL 测试、类型检查、API 构建通过。覆盖并发重试只执行一次、双 Worker 串行、跨案件隔离、权限撤销、事务回滚、中断保留内容、孤儿任务不重放、思考与工具的顺序及持久化。
- `npm run agent:build` 与 `npm run preview:build` 通过；上游解析器新增的 DeepSeek reasoning 事件测试通过，断言思考不会混入正文。Rust 构建保留上游已有 dead-code 警告。
- 真实案件读取与 todo 工具调用完成。JCode 会话 `session_fd1055d7cc1c42ecbe2c2fbe417d075b` 的 todo、消息、工具结果存入 PostgreSQL。API 进程重启后，新 Bridge 从同一会话正确恢复合成代号与已完成待办。
- 真实浏览器对话中，案件 `ef738a6c-d920-419f-ac8c-1015dcb46aaf` 的首轮依次产生思考、`case_read`、思考、正文；三个步骤均保存。最终正文 420 字符；浏览器捕捉到多次递增的可见文字长度和输入光标，桌面 1440px、移动端 390/320px 无页面横向溢出，输入框保持在视口内。
- 运行中检查临时目录未发现 session/todo JSON/JSONL 文件，结束后目录清理。模型密钥和存储令牌不进入 Web；私有诊断脱敏。

本机证据位置（临时文件，不代替版本化测试）：

- 首轮真实工具、todo 和重启：`/tmp/erp-deepseek-live/report.json`。
- 思考和正文流式帧：`/tmp/erp-reasoning-live/report.json`。
- 刷新、键盘、快速整段返回及减少动态效果：`/tmp/erp-reasoning-live/edge-report.json`。
- 桌面展开截图：`/tmp/erp-reasoning-live/desktop-expanded.png`；输出中截图：`desktop-typing.png`；手机截图：`mobile-reasoning-detail.png`。
- `typing-replay.webm` 为浏览器回放已保存真实回复的组件录像；该回放不是额外一次模型性能测量。

## 时延样本及限制

初始真实 todo 回合，按轮询观察的首个回答约 4251 ms、结束约 4960 ms；重启后的记忆追问结束约 2201 ms。带思考展示的新案件回合，首次观察思考约 1543 ms、回答约 2860 ms、结束约 3618 ms。样本包含排队、进程和轮询开销，不是精确模型 TTFT，也不构成 P95、冷暖启动或容量基准。

原生接口工具探针发现：当前 thinking 模式不支持强制选择指定 `tool_choice`，返回 400；正常自动工具选择以及函数结果继续请求均返回 200。产品路径使用自动工具选择。接口依据：[官方 Responses 指南](https://api-docs.deepseek.com/guides/responses_api/)、[创建响应契约](https://api-docs.deepseek.com/api/create-response/)。

当前生产回归基线仍是宿主机独立进程；Cube resident Runtime Bridge 已接入 Worker 的显式 `AGENT_EXECUTION_BACKEND=cube` 路径，首条消息自动创建案件实例，后续复用同一常驻 JCode/Bridge。默认仍是本地独立进程，避免未注册模板时静默切换。单 Worker 同时执行一个回合，默认无总时限（可通过 `AGENT_TURN_TIMEOUT_SECONDS` 显式设置），输出预算 4096 tokens。流式正文检查点、断流/强杀恢复和超过 150 秒的受控回合已通过；真实 Cube 模板注册与重建恢复、ERP/知识 MCP 端到端回合、取消交互、预热和混合负载仍待验证。
