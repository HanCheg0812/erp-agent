# API 职责边界

日期：2026-09-22，测试补齐于 2026-09-23。状态：HTTP 适配层与案件、会话、工具权限、原件/记忆职责拆分已通过本地回归；全项目 DDD 分层未完成。

## 范围与决定

用户指出把根目录文件改为领域目录、再把代码放进 `mod.rs` 或 `handlers.rs`，没有解决职责过载。本轮延续已有目录，不调整 API 路径、数据库模式、部署拓扑或外部 Harness 位置。

- `mod.rs`：模块声明与选定导出，无业务函数。
- `handlers.rs`：提取 HTTP 输入/身份、解析 Bearer、调用用例、映射状态码和响应。无 SQL、事务、业务角色判定或外部服务调用。
- 案件 `application/`：创建、上下文补全、提案、审批与查询；负责业务授权、调用规则、开启/提交事务。
- 案件 `domain.rs`：输入规范化、版本与状态迁移、方案校验、审批意图摘要。
- 案件 `repository/`：记录、查询、方案、审批和审计的 SQL 与锁操作。由上层用例控制提交。

其余模块按实际职责拆分，避免把全部实现集中到同一个 service：

| 模块 | 当前内部职责 |
| --- | --- |
| auth | accounts、passwords、sessions、principal |
| conversation | application（history/settings/messages/cancellation）、domain（操作、重试、停止规则）、repository（queries/settings/messages/runs） |
| permissions | application（actions/decisions/session/recovery）、domain（绑定、决策、过期）、repository（同名持久化职责）、reviewer 外部适配 |
| agent_sessions | registration、leases、storage、documents、receipts |
| integrations | settings、gateway、ERP、knowledge、response、调用类型 |
| objects | application（documents/memory）、domain（上传校验与重试匹配）、repository（documents/memory）、RustFS storage |
| sandboxes | lifecycle、provider、reconciliation、repository、sweeper |

## 保持的业务约束

审批摘要保持原序列化字段顺序，避免已有幂等请求因拆分失效。案件版本锁、审批幂等锁及事务内审计保持；Agent 业务工具调用事务内版本的案件用例，让权限执行记录和案件变更共同提交或回滚。

Cube 保留持久创建意图、远端丢响应后的核对和删除确认。Agent 会话仍只使用 PostgreSQL 存储，保留版本比较、租约 fencing、原始内容及重试回执。RustFS 上传仍保留 uploading/failed/ready 阶段与哈希校验。

## 本次继续优化

- conversation、permissions、objects 的全部 application 文件不再执行 SQL；repository 按持久化职责分文件，不聚集到一个大 repository 函数。事务提交仍由用例完成，查询隔离级别、锁顺序及条件更新保持。
- 对话操作使用 `Operation` 枚举区分消息和压缩。重试绑定正文、设置和操作类型；压缩重试取原记录设置，不受后来修改会话设置影响。运行中的取消仅请求停止，等待 Worker 检查点，不能直接标为已结束。
- permissions 不再直接写个人记忆表，通过 `objects::save_agent_memory_tx` 加入原权限事务。通用工具批准仍不能代替 exact-version 人工业务签字；Agent 案件读取复用案件范围查询，不复制案件详情 SQL。
- objects 的上传解码/路径/类型/大小和重试指纹检查独立为规则，S3 网络请求归 storage 适配器。持久化保存 uploading/failed/ready 阶段，下载仍验证 SHA-256。
- 源码边界测试扩展到上述 application 与 repository：禁止用例直接执行查询和引入 HTTP extractor，禁止 repository 提交事务、获取连接、执行角色判定或调用 HTTP。此检查是回退护栏，不是 Rust 编译器级架构约束。

## 本地证据

- `cargo test --locked --manifest-path services/api/Cargo.toml -j 6`：2026-09-23 共 46 项模块/架构测试及 3 项 `services/api/tests/` Router 集成测试通过。除原有案件、会话、权限、原件规则外，新增认证、Bearer/密码/token/角色、Agent Session delta/document、ERP/OpenViking 参数、上游响应、平台基础函数、Cube 元数据/节点、Worker catalog 与流事件测试。
- `npm run check`：Web 类型检查、API/MCP 构建、真实本地 PostgreSQL 独立测试库集成测试、Web 构建通过。API 单元/边界测试已加入该常规检查链。
- [API 测试矩阵](../../../../services/api/TESTING.md)逐项登记全部 10 个顶层 API 模块的直接测试与集成证据；源码测试会检查矩阵没有遗漏顶层模块。
- 新增集成断言：带首尾空白的方案摘要正常规范化；审计失败时上下文、方案版本和审批全部回滚；失败的审批幂等键可在故障移除后正常重试。
- 新增 `tests/system/test-layer-boundaries.mjs`：真实测试数据库 + 受控 S3 HTTP 传输 fixture 验证原件上传幂等、失败后恢复、哈希错误拒绝和跨用户隔离；不访问开发 RustFS bucket。
- 新增故障注入：Agent 记忆写入后审计失败时，记忆和权限执行记录一起回滚；恢复后同请求正常执行且重复请求不重复写入。记忆容量满时，Agent 409 与用户入口 400 保持原契约。
- 新增压缩回归：修改会话设置后重试旧压缩请求，仍返回原消息与原设置，不覆盖最新会话设置。
- 既有并发审批、租约失效、跨用户隔离、API 重启、Worker 停止/检查点、权限恢复、业务动作和压缩回归均通过。

集成检查使用真实本地 PostgreSQL、受控 Runtime/Cube/ERP/知识服务 fixture，不代表重新验收真实外部服务。本次验证了受控 S3 fixture，但未重新进行真实 RustFS 上传下载、真实模型或 Cube 内 Harness 验收，未重启日常运行的开发 API，未提交、推送或部署。

## 明确剩余项

这不是完整的持久化无关领域模型。auth、agent_sessions、integrations、sandboxes 等用例仍有直接 SQL，领域校验仍依赖共享 API 错误类型及 JSON 值，跨领域查询仍通过现有兼容接口与事务类型组合。后续在各模块内部继续抽离 repository 和类型化领域错误，不能把这些耦合藏进另一个通用大函数。平台启动和 Worker 执行循环不在本轮 HTTP 层拆分范围内。

源码边界和测试矩阵检查是防止明显回退的护栏，不替代覆盖率工具、编译期依赖约束、代码审查和行为测试。这里的“全部模块有测试”指每个模块有对应的规则或集成行为证据，不代表每一行 SQL 都有一条一对一测试。结构拆分不等于生产性能或业务集成验收。
