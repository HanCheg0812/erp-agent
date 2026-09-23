# JCode 会话持久化

设计日期：2026-09-21，进展更新：2026-09-22。用户确定要求：JCode session 等持久状态存储在 PostgreSQL，不使用本地 JSON/JSONL 文件作为正式存储。远程存储补丁和 Rust API 的 `agent_sessions`、有序消息/事件流、文档、运行记录表已实现；真实消息、工具、todo、长上下文压缩及主机进程重启恢复通过。证据见[模型联调](../plantree/plans/server-demo/deepseek-integration.md)与[Rust/压缩验收](../plantree/plans/server-demo/rust-auth-context.md)。Cube 重建、常驻 Harness 和吞吐仍待实测。

## 责任边界

PostgreSQL 是会话的唯一持久来源，沙箱只保存运行中内存、socket、临时附件和可重建缓存。不能采用“JCode 继续写 JSON，后台定期同步数据库”的双写方案；Cube 快照也不是会话数据库。沙箱销毁后必须能在新实例恢复已提交会话。

业务数据库已有的 `sessions` 表是登录令牌表，不是 JCode 会话表。新增业务域使用 `agent_sessions/` 和 `agent_*` 表名，避免混淆。所属租户、用户、案件和运行实例绑定由后台生成，模型不能修改归属或指定其他用户的 session。

建议 Rust 持久化接口对接受限的内部会话存储服务，由后台连接池访问 PostgreSQL；Cube 中不放业务库的通用账号。存储访问不作为模型可调用的 MCP 工具。外部 Harness 也使用同一存储契约；存储位置不依赖最终 Cube 放置决定。

## 已检查的源码

固定 JCode commit：`2a4edaa02057ac994a601311c4f03ed450e1b3c9`。

| 源码位置（相对 external/jcode） | 当前行为 / 适配范围 |
| --- | --- |
| `crates/jcode-base/src/session/persistence.rs` | `save`、`load`、`load_from_path`、启动简读、远程恢复；目前读取 JSON 快照并重放 journal |
| `crates/jcode-base/src/session/journal.rs` | 追加消息、运行元数据、事件及检查点状态 |
| `crates/jcode-base/src/session.rs` | Session 含 messages、compaction、provider session/model 等恢复所需字段 |
| `crates/jcode-base/src/session/storage_paths.rs` | 按文件路径判定会话存在；服务器模式必须改为存储查询 |
| `crates/jcode-base/src/session/crash.rs` | 扫描本地目录发现和恢复会话，服务器模式不能继续以目录为准 |
| `crates/jcode-base/src/recent_session_index.rs` | 本地 SQLite 最近会话索引，应转为数据库分页查询 |
| `crates/jcode-app-core/src/tool/session_search.rs` | 扫描快照/journal 搜索；首版关闭跨会话搜索，启用时必须后端鉴权 |
| `crates/jcode-app-core/src/tool/todo.rs` | todo 使用独立保存接口，必须继续追踪并接入数据库或替换为业务 todo 工具 |

因此不能只在 Web 保存对话文本，也不能只替换一个 JSON 写入函数。要审查 fork、删除、标题更新、上下文压缩、列表、历史读取和恢复入口。未适配的文件存储入口在产品服务器模式下明确拒绝，而非静默回退。CLI 上游模式可保留文件存储，但产品启动必须显式强制 PostgreSQL 存储，缺配置即失败。

## 数据与性能

会话元信息、归属、版本、消息顺序、工具调用 ID、运行状态使用关系字段；工具参数、消息内容块和版本化压缩检查点可使用 JSONB。JSONB 是数据库里的结构化载荷，不是把整个 session 文件反复写回一个大字段。

建议分离 `agent_sessions`、`agent_messages`、`agent_tool_calls`、`agent_checkpoints`、`agent_events`、`agent_todos`。模型/provider 恢复字段、usage、压缩上下文、待处理工具调用必须一起纳入契约；附件正文另存文件存储，数据库保存引用。表结构在实际 Rust 接口适配时确定，当前不创建没人读写的占位表。

消息和事件按会话递增序号追加；压缩保存检查点及其覆盖序号，不删除原始历史。流式回复还使用独立的 `stream_checkpoint` 文档：它记录最后一条已提交消息的精确锚点和当前正文，正文在发送到 Web 前写入 PostgreSQL，正常完成后清空。新进程只在锚点仍匹配时恢复，并在恢复后清空文档，因此重复重启不会重复追加。元数据查询不加载全文；只加载检查点和必要尾部消息，避免启动时扫描所有用户会话。

流式 token 的可见正文先写入有界检查点，再发送给 UI；UI 可以显示打字过程，但重启后只恢复已提交的正文。用户输入在模型运行前持久化，工具意图在执行前持久化，工具结果在进入下一模型步骤前持久化。不能只在整个 turn 结束后保存。数据库不可用时停止推进不可恢复的动作，不能回退 JSON。每个 token 一次读写目前作为正确性基线，性能优化必须以真实 TTFT、吞吐和数据库等待测量为依据。

每个活跃会话持有后端授予的单写者租约及递增 fencing token，写入还校验期望版本、事件 ID 与幂等键。暂停恢复后的旧进程必须重新获取租约，不能凭快照中的旧凭据覆盖更新状态。冲突必须停止并重新读取，而不是 last-write-wins。

JCode 当前部分存储接口是同步函数，适配时要明确异步 I/O 或有界存储执行线程，避免在共享 Tokio 执行线程上做阻塞远程请求。连接复用、批量追加和读取缓存均要受版本约束；不能用每 token 一次 HTTP/SQL 写入换取表面上的实时性。

## 验收

1. 从创建、消息、工具结果、todo 到压缩/恢复，全程没有业务会话 JSON/JSONL 落盘；配置与临时文件另计。
2. 销毁原沙箱和工作目录，从 PostgreSQL 在新实例恢复上下文、todo、已完成工具结果与事件游标。
3. 暂停旧实例后新实例接管，旧实例复活的写入被 fencing token 拒绝。
4. 重复消息、重复结果、超时和断线恢复不会生成重复业务动作；ERP 已执行但结果未记账时先对账。
5. 跨租户/案件查询和伪造 session ID 被拒绝；克隆模板不复制用户身份与会话归属。
6. 对比追加/检查点持久化的 P95、吞吐、连接池等待与冷恢复耗时；不以牺牲已确认写入的持久性换速度。

当前 Rust API 集成测试覆盖存储 CAS/fencing、重试、跨案件鉴权、并发 worker、中断状态、权限重检和迁移 readiness；真实 JCode 探针补充消息/工具/todo、压缩、原始历史保留、对象哈希及进程重启证据。两者仍不能代替上述 Cube 重建、常驻 Harness 及吞吐验收。
