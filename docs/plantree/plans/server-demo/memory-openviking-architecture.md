# 记忆与 OpenViking 架构

状态：P0/P1 基础实现已落地；文档 RAG 的持久分块/embedding 队列已落地，真实 TEI、向量召回、OpenViking 发布和混合负载尚未验收。

2026-09-24 实现进展：PostgreSQL Compose 已切换到 `pgvector/pgvector:pg17-bookworm`；个人记忆在现有 `user_memories` 兼容表上增加作用域、生命周期、版本、来源哈希、词法列和 `memory_outbox`，但 `PERSONAL_MEMORY_EMBEDDINGS_ENABLED` 默认关闭，因此普通记忆只走 PostgreSQL 词法召回，不产生 embedding 任务；创建/撤回写入独立 `memory_audit_events`。API 与 Agent 事务都使用同一记忆锁，Worker 按当前消息做 exact/substring/`tsvector` 召回，最多注入 12 条。文档侧新增 `knowledge_chunks`、`knowledge_embedding_jobs` 和 pgvector 表，独立 Rust worker 调用 TEI、按租约和 fencing token 恢复、按文本哈希幂等写入；它不消费 `memory_outbox`。旧的最小字段直写仍按 `owner_id` 回退读取。真实 TEI、解析产物入队、向量检索/OpenViking 投影和 token 精确预算仍待后续阶段。

## 1. 目标

云端 Agent 不把 JCode 的本地 JSON 记忆当作正式存储。JCode 负责运行时上下文和模型工具循环；PostgreSQL 负责长期记忆的权威状态；OpenViking 统一承载企业资料、目录、Wiki 派生页和 RAG 检索。

本设计同时解决四件事：

1. 多实例运行时可以恢复同一用户的记忆和会话，不依赖本地磁盘。
2. 个人记忆、客户共享经验、企业资料和 ERP 实时事实不混为一类。
3. OpenViking 的 Wiki、分层上下文和 RAG 使用同一资源树，不再维护独立 Wiki 服务。
4. 记忆撤回、权限变更和文档修订不会因为搜索索引延迟而继续注入模型。

## 2. 总体边界

```text
用户 / Agent
    -> Rust API（身份、范围、审批、版本、审计）
        -> JCode Runtime（无状态运行时）
        -> Memory Gateway -> PostgreSQL（权威记忆与 pgvector）
        -> Knowledge Gateway -> OpenViking（Resource / Wiki / RAG）

原件 -> RustFS -> 解析 Worker -> PostgreSQL 解析产物与定位
                                      -> OpenViking 资源索引
```

### PostgreSQL 是控制面和记忆权威

PostgreSQL 保存记忆正文、作用域、生命周期、审批、来源、版本和审计。它也可以保存个人记忆的词法索引与向量索引。OpenViking 或其他检索服务返回的结果不能改变这些状态。

### OpenViking 是上下文面

OpenViking 保存企业资料的可检索资源、L0/L1 导航摘要、L2 详情和经批准发布的 Wiki 派生页。它负责语义检索和按需深读。其内置 user memory 与 session commit 首期关闭，不复制 JCode 会话，也不直接接受 Agent 的 `remember`、`write` 或 `forget`。

### JCode 是无状态运行时

JCode 不拥有长期记忆。会话消息、工具结果、todo、压缩和恢复状态继续按[会话存储约束](../../../architecture/session-storage.md)只写 PostgreSQL。每轮由 Rust 后端计算记忆召回结果，再以不可信上下文注入 JCode。只有在明确启用 `PERSONAL_MEMORY_EMBEDDINGS_ENABLED=1` 并完成独立模型/策略验收后，个人记忆才进入可选 embedding outbox。

## 3. 信息分类

| 信息 | 权威来源 | 检索方式 | 是否进入长期记忆 |
| --- | --- | --- | --- |
| 用户偏好、交互习惯 | PostgreSQL `memory_versions` | PG 词法 + pgvector | 是，需用户确认 |
| 客户经验、纠错和约定 | PostgreSQL，客户授权范围 | PG 过滤后召回；必要时投影到 OV | 是，需授权角色确认 |
| 需求单、规格书、图纸、表格 | RustFS 原件 + PG 解析版本 | OpenViking Resource/RAG | 否，作为资料 |
| Wiki 页面 | OpenViking 派生 Resource，PG 发布记录 | OV 目录、L0/L1、find/read | 否，必须链接原始资料 |
| 订单、库存、工单状态 | ERPNext | 每次调用 ERP 工具 | 否 |
| 当前案件事实 | PostgreSQL 案件与会话 | 当前案件上下文 | 默认不跨案件提升 |

客户共享经验不能放入个人 `user/.../memories` 空间；它属于租户/客户范围的受控资源。OpenViking ACL 只能作为额外保护，平台 EBAC、客户授权和当前案件范围仍由 Rust 后端裁决。

## 4. 记忆数据模型

当前 `user_memories` 只有 owner、正文和创建时间，适合原型，不足以支持云端版本和撤回。目标模型是追加式版本，不原地编辑：

```text
memory_items
  id, tenant_id, scope_kind, scope_id, category
  current_version_id, created_at, updated_at

memory_versions
  id, memory_id, version, text, normalized_text
  status: proposed | active | superseded | rejected | retracted
  valid_from, valid_to, review_after
  source_kind, source_id, source_hash
  approved_by, approved_at, approval_hash

memory_embeddings
  memory_version_id, model_id, dimensions, metric
  embedding, embedded_text_hash, created_at

memory_outbox
  event_id, memory_version_id, operation, attempt_count
  state, last_error, created_at, processed_at
```

所有读取都先限定 `tenant_id`、`scope_kind/scope_id`、`status=active` 和有效期。编辑生成新版本并使旧批准失效；撤回先在 PostgreSQL 生效，再异步清理任何检索副本。

### 写入流程

```text
用户消息
  -> Agent 提议 memory
  -> Rust API 校验长度、类别、来源和作用域
  -> proposed
  -> 用户/授权角色批准精确文本
  -> PostgreSQL 事务 active + audit + outbox
  -> Worker 建立 PG embedding
  -> 可选：把已批准版本投影到 OpenViking
```

资料内容、工具结果和资料中的指令不自动变成个人记忆。后续可以增加离线候选提取，但只能产生 `proposed`，不能直接发布。

### 读取流程

```text
当前用户消息 + 案件上下文
  -> Rust 后端推导允许的记忆范围
  -> PG exact / lexical / vector 召回
  -> 事务内复核 active、有效期、权限和版本
  -> 按 token budget 选择少量结果
  -> 以不可信 memory context 注入 JCode
  -> 记录 memory_version_id 和注入原因
```

记忆只影响回答方式和分析提示，不能作为数量、尺寸、修订或 ERP 状态的唯一依据。

## 5. pgvector 选择

PostgreSQL 本身没有原生向量类型，这里指安装 `pgvector` 扩展后的能力。官方 README 区分存储维度和可建索引维度：`vector` 存储类型最多 16,000 维，但 HNSW/IVFFlat 的 `vector` 索引最多 2,000 维；`halfvec` 索引最多 4,000 维；二值量化可索引最多 64,000 位；`sparsevec` 适合稀疏非零元素场景。[pgvector 类型与索引限制](https://github.com/pgvector/pgvector#hnsw)

因此首版不把模型维度写死在业务表中：

1. `embedding_models` 登记模型名、版本、维度、距离度量、存储类型和归一化规则。
2. `memory_embeddings` 保存 `model_id`，不同模型不混用同一个索引。
3. 首选 1536 维的普通 `vector`，因为可以直接使用 HNSW/IVFFlat。
4. 若选用 3072 维模型，使用 `halfvec(3072)` 索引或先评测降维/子向量；不能直接假定普通 `vector(3072)` 可以建标准索引。
5. 只有个人记忆和规模可控的资料块先进入 PG 向量索引；大规模企业资源仍以 OpenViking 作为检索对照和候选主引擎。

PG 检索不是因为“维度越高越好”。最终选择由中文工业资料的逐题 Recall、引用定位、过滤召回、P95 和 embedding 成本决定。高维度不解决错误作用域，也不能替代版本过滤。

## 6. OpenViking 资源树

企业资料使用不可变版本路径，发布指针在 PostgreSQL：

```text
viking://resources/{tenant}/{customer}/{project}/
  parts/{part}/{revision}/{document_version}/
    .abstract.md
    .overview.md
    source/...
    blocks/...

viking://resources/{tenant}/{customer}/wiki-drafts/{draft_id}/...
viking://resources/{tenant}/{customer}/wiki-published/{page_version}/...
```

解析 Worker 先把原件、页码、区域、单元格和父子关系写入 PostgreSQL，再把规范化文本与结构导入 OpenViking。命中 URI 回到 PG 映射为稳定 `block_id` 和原件定位；OpenViking 的摘要本身不能充当数字事实的最终引用。

Wiki 是同一资源树中的派生层：

- L0/L1 提供目录导航和初步相关性判断。
- `find` / `read` 提供 RAG 的检索和深读。
- `ov compile` 只写入 draft 路径。
- 发布前检查来源块、关键字段和适用版本。
- 原始资料撤回或修订后，相关 Wiki 页面自动失效。
- 已发布 Wiki 不作为下一次编译的唯一输入，避免摘要递归污染。

## 7. OpenViking 与 PG 的关系

首期采用以下模式：

| 能力 | 首期权威 | 首期检索 | 后续选择 |
| --- | --- | --- | --- |
| 个人记忆 | PostgreSQL | PG lexical + pgvector | 评测后可投影到 OV user memory |
| 客户经验 | PostgreSQL | PG 过滤后召回 | 必要时投影到 OV 共享 Resource |
| 企业资料 | RustFS + PostgreSQL 发布状态 | OpenViking | 与 PG 基线对照 |
| Wiki | PG 发布清单 + OV 内容 | OpenViking | 继续使用 compile 或改为人工模板 |

这不是要求两个系统同时决定事实。PG 是 source of truth；OpenViking 是可重建的上下文索引。OpenViking 不可用时，记忆仍可由 PG 的 exact/lexical 查询工作；知识检索可以明确返回未配置或降级状态，不能静默注入过期资料。

## 8. 实施阶段

1. **P0：记忆云端化**。扩展 `user_memories` 为追加式版本模型；建立范围、审批、撤回、来源和 outbox；JCode 禁用本地 memory sidecar；继续保留会话 PG 唯一持久化。
2. **P1：PG 记忆召回**。安装带 pgvector 的 PostgreSQL 镜像；锁定一个 embedding 模型；实现 exact、中文 lexical、vector 三路候选和 token budget；完成用户/案件隔离与撤回测试。
3. **P2：OpenViking 资料链路**。MinerU/视觉解析、统一 block locator、版本路径、资源导入、`find/read` 适配和证据回查；不启用 OpenViking session memory。
4. **P3：Wiki 与记忆投影评估**。用相同样本比较 PG 记忆召回、OpenViking 分层召回和可选记忆投影；只有在跨层导航或长文本案例有明确收益时，才启用投影。

## 9. 验收门槛

- API、Worker、JCode 进程重启后记忆、审批、outbox 和召回可恢复。
- 同一用户不同案件、不同用户和不同租户之间召回为零泄露。
- `retracted`、`superseded` 和过期版本不再注入，即使 OpenViking 尚未完成删除。
- 记忆候选不会改变工具权限、审批状态或 ERP 写入条件。
- 记忆注入不超过预算，并记录版本 ID、来源和召回原因。
- 企业资料的引用可以回到页、区域、表格单元格或图片坐标。
- PG 与 OpenViking 使用相同解析产物和 embedding 的对照结果单独记录，不把模型变化误判为检索收益。

## 10. 当前待实测

- `pgvector` 镜像、扩展安装、HNSW 带过滤召回和真实中文资料效果。
- 选定 embedding 模型的维度、中文/工业料号表现、价格和 P95。
- OpenViking 固定 commit 的资源导入、`target_uri`、ACL、删除对账和引用回查。
- 个人记忆是否在第二阶段投影到 OpenViking，以及投影是否能带来可解释的召回收益。
