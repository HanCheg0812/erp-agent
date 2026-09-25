# 知识检索与长期记忆方案调研

调研日期：2026-09-24。背景：用户希望替换当前 RAG 方案（候选为开源 Knowhere），同时要求必须有记忆，并追问 OpenViking 本身能否承担知识检索、LLM Wiki 能否替代 RAG。本文为调研与建议：只读核查固定 commit 的源码与文档，未部署、运行或评测任何候选，也未改变[已确认决策](decisions.md)。

相关：[RAG 接入方案](rag-foundation.md)、[开源实践调研](rag-references.md)、[Rust 后端与上下文](rust-auth-context.md)、[解析流水线设计](parser-pipeline-design.md)。

## 结论

一句话：**云端记忆以 PostgreSQL 为权威状态并可使用 pgvector 召回；企业资料、Wiki 和 RAG 继续统一进入 OpenViking 的 Resource 上下文；Knowhere 不作替换件，只借鉴设计；LLM Wiki 不另建服务，而作为 OpenViking 中可追溯的派生资源。** 本文早先提出的“PG 作为 RAG 默认骨干”保留为对照方案，不再覆盖已确认的 OpenViking 知识底座方向；实际检索骨干仍需用相同解析产物和 embedding 实测。

总原则：事实只来自两处，即原件（解析后可定位的块）与 ERP。摘要、Wiki 页和记忆都是带来源链接的派生物或提示，不作为数量、修订、尺寸或 ERP 状态的依据。

1. **记忆与 RAG 是两个独立决策。** 项目中有五类信息，权威来源各不相同（第 1 节）。Knowhere 所说的 document memory 实际是文档知识库，不含用户记忆；OpenViking 的 memory 是 LLM 自动整理的 Agent 上下文。两者都不是本项目需要的“经审批、可追溯、按身份限定范围”的个人/客户记忆。
2. **长期记忆：升级现有 `user_memories`，不采用 OpenViking、JCode 上游或第三方记忆服务。** 这一结论不随 RAG 选型变化。现有“模型提议 → 挂起审批 → PG 保存 → 标记不可信后注入”的方向正确，缺的是召回与 token 预算、客户范围、状态/取代关系、来源和评测（第 5.1 节）。
3. **OpenViking 确实能做 RAG**：资源导入、L0/L1/L2 分层摘要、向量索引和限定 `target_uri` 的 `find` 都具备。但项目已决定由 Worker 先解析、由后端裁决业务范围与版本，它剩下的角色只是“索引 + 检索”。在这个角色上，自托管默认只有稠密召回，不利于精确料号/图号；范围只能用路径前缀和 tag 等值表达；命中不带原文定位；删除跨两个存储、非原子；还需额外运维一套 AGPL 单副本服务。它的“RAG + 记忆一体化”在这里不成立，因为记忆那一半与项目边界冲突（第 2.2 节）。
4. **Knowhere 不适合作为 RAG 替换件**：只有词法检索、没有向量；引用只到章节路径与页码，没有区域/单元格；只有软删除、任务无幂等键；MinerU、Agent Harness 与遥测默认走云端；自报经典检索 P50 约 11 秒；建仓约 5 个月；Worker 依赖 AGPL 的 PyMuPDF。值得借鉴的是：范围在 SQL 候选阶段收窄、hydration 再复核，章节路径与正文双通道 RRF，以及可导航的语料工具。“用 Agent 读文档替代一次性 top-k”的思路，在本项目由 JCode 调用受控深读工具实现，不必在知识服务里再嵌一个 Agent。
5. **推荐默认路线：自有解析规范化 + PostgreSQL 原生混合检索**（pgvector、规范化标识精确匹配、中文全文，在同一 SQL 内先按业务范围收窄再召回）。理由：业务边界本就必须在 PG 实现；[rag-foundation](rag-foundation.md) 第 4 步本就要求“基础分块向量检索”基线；AGENTS.md 要求没有已证明的需要时不拆分更多服务。OpenViking 用同一解析产物和 embedding 参加对照，明显胜出并通过 AGPL 评审后才接入同一适配层（第 5.3 节）。
6. **LLM Wiki 不作为 RAG 替代。** 它可以取代“切块 + 单次 top-k”的用法，但不能取代原文取证。这里指 Karpathy 模式，OpenViking 的 `ov compile` 已内置同类能力。本项目吸收它的两点：一是确定性目录，相当于 `index.md`，由 PG 元数据生成、不经 LLM；二是“知识沉淀”，落在第 5.1 节的审批记忆条目上，客户/料号知识页只是这些条目与资料目录的渲染视图。由 LLM 全权改写的 Wiki 与本项目的修订/有效期裁决、按范围授权、撤回传播和页/区域引用要求冲突。两篇 2026 年预印本显示：未经校验的编译会丢失关键事实；分解式检索能以更低 token 成本拿回 Wiki 大部分跨文档综合优势（第 2.7 节）。自动编译等评测证明跨文档综合题确有缺口后，再以离线提议加审批的方式试点（第 5.4 节）。
7. **待用户确认**：是否据此调整 AGENTS.md 的 Accepted Direction、决策 5/12/26 和 rust-auth-context 第 7 条；记忆范围 A/B；`memory_save` 在“完全访问”下是否仍需人工确认；Wiki 是否按本文只做受控视图。见第 7 节；本文未修改这些文件。

## 1. 问题拆分

| 类别 | 例子 | 权威来源 | 当前状态 | 本文建议 |
| --- | --- | --- | --- | --- |
| 企业资料（RAG） | 需求单、扫描件、表格截图、照片、图纸、规格书 | 原件及 PG 中已发布版本 | RustFS 原件已接通；解析与检索未实现 | 第 5.2 节 |
| ERP 实时事实 | 库存、订单、工单状态 | ERPNext | ERP MCP 只读已接入 | 每次经 ERP 工具读取，不进入记忆或知识索引 |
| 会话/工作记忆 | 本会话消息、工具结果、todo、压缩 | PostgreSQL（JCode 会话存储） | 已验证 | 保持 |
| 长期记忆 | 个人偏好、客户经验与纠错 | 本人确认或授权角色批准的记录 | `user_memories` 仅个人范围 | 第 5.1 节 |
| 程序性知识 | 业务流程与核对清单 | 版本化 Skill | 已接入 | 组织级流程写成 Skill，不进记忆 |

案件事实留在案件上下文，不自动升级为跨案件记忆（沿用 rust-auth-context 第 7 条）。

## 2. 候选核查

### 2.1 OpenViking 资源与检索

版本 `2562688c`（2026-09-24 HEAD）。

| 核查结论 | 对本项目的影响 |
| --- | --- |
| PDF `auto` 只在本地解析抛异常时才转 MinerU（[pdf.py](https://github.com/volcengine/OpenViking/blob/2562688c050e72fa1a6526732cec1139566677e9/openviking/parse/parsers/pdf.py#L201-L214)）；MinerU 适配为同步 `/file_parse`，只取 `md_content` 与图片（[pdf.py](https://github.com/volcengine/OpenViking/blob/2562688c050e72fa1a6526732cec1139566677e9/openviking/parse/parsers/pdf.py#L715-L753)），不保留 content_list/middle_json 的页码与坐标 | 扫描件在 `auto` 下可能静默得到极少文本；无法由它产出区域引用。继续由 Worker 先解析 |
| 图片语义描述标为 Experimental；大图原件 “intentionally NOT saved”（[image.py](https://github.com/volcengine/OpenViking/blob/2562688c050e72fa1a6526732cec1139566677e9/openviking/parse/parsers/media/image.py#L202)）；OCR 仅在文档字符串中提及，未见实现 | 原件、OCR 与坐标变换必须留在业务侧 |
| XLSX 经 anydoc 转 GFM 表格，默认截断 1000 行（[anydoc_converter.py](https://github.com/volcengine/OpenViking/blob/2562688c050e72fa1a6526732cec1139566677e9/openviking/parse/parsers/anydoc_converter.py#L66)），合并单元格为空、无单元格坐标 | XLSX 使用已有 [calamine 抽取](../../../../services/ingestion-worker/src/extract.rs)保留单元格 |
| L0/L1/L2 摘要按文件/章节调用 VLM/LLM；`processing_mode=vectors_only` 可跳过，但也失去分层优势 | 分层是它相对普通分块的主要差异；评测同时记录收益与入库成本 |
| `find` 默认稠密向量；稀疏/混合依赖支持 sparse 的 embedding（[配置文档](https://github.com/volcengine/OpenViking/blob/2562688c050e72fa1a6526732cec1139566677e9/docs/en/guides/01-configuration.md)以 Volcengine 为例）；`grep` 是独立工具，`auto` 引擎在节点数超过 `switch_to_remote_threshold`（默认 10000）时才转远端 BM25（[_grep.py](https://github.com/volcengine/OpenViking/blob/2562688c050e72fa1a6526732cec1139566677e9/openviking/storage/viking_fs/_grep.py#L74-L84)），不与 `find` 融合 | 料号、图号等精确标识召回需另行实现 |
| `find`/`search` 支持 k=v tag 等值过滤（[search.py](https://github.com/volcengine/OpenViking/blob/2562688c050e72fa1a6526732cec1139566677e9/openviking/server/routers/search.py#L76-L86)）与 `target_uri`，context 模式拒绝 `target_uri`（[search.py](https://github.com/volcengine/OpenViking/blob/2562688c050e72fa1a6526732cec1139566677e9/openviking/server/routers/search.py#L251)）；自定义字段无范围条件 | 客户/料号/修订须编码为路径或 tag；有效期等范围条件只能由后端预先换算为 URI 集合 |
| 命中 `MatchedContext` 只有 uri、context_type、level、abstract/overview、category、score、match_reason、search_tags（[types.py](https://github.com/volcengine/OpenViking/blob/2562688c050e72fa1a6526732cec1139566677e9/openviking_cli/retrieve/types.py#L277-L289)）；`include_provenance` 是检索轨迹，不是原文位置 | 页/区域/单元格由 URI 回查 PG 中的块与坐标 |
| `rm` 加路径锁后先删向量记录再删文件（[_ops.py](https://github.com/volcengine/OpenViking/blob/2562688c050e72fa1a6526732cec1139566677e9/openviking/storage/viking_fs/_ops.py#L215-L230)），无跨存储事务和原子版本切换；导入走 SQLite/Redis 持久队列，重启后处理中任务回到 pending | 每个修订独立路径，由 PG 发布指针切换并异步清理、对账；上游队列与 PG 任务表是两套状态 |
| trusted 模式配置 root key 后，请求只需带该 key；account/user 取自请求头，可用 `X-OpenViking-Role` 声明 admin，admin 路径解析为 ROOT（[trusted.py](https://github.com/volcengine/OpenViking/blob/2562688c050e72fa1a6526732cec1139566677e9/openviking/server/auth/plugins/trusted.py#L125-L218)）；未知身份默认每 300 s 批量自动注册（[config.py](https://github.com/volcengine/OpenViking/blob/2562688c050e72fa1a6526732cec1139566677e9/openviking/server/config.py#L342)） | 一个密钥即可冒充任意账户；只能作为仅后端可达的内部组件，授权完全在我们后端 |
| `/mcp` 固定挂载，同时提供 write/edit/forget/add_resource 等写入与删除工具，无只读模式，检索工具不接受 tag/过滤参数（[mcp_endpoint.py](https://github.com/volcengine/OpenViking/blob/2562688c050e72fa1a6526732cec1139566677e9/openviking/server/mcp_endpoint.py)） | 不向 Agent 或网络暴露。现有[适配](../../../../services/api/src/integrations/knowledge.rs)实际调用 REST `search/find` 与 `content/read`，而非决策 12 字面上的 HTTP MCP，建议保持 |
| 无 PostgreSQL/pgvector 后端（ragfs 的 sqlx 只启用 sqlite/mysql，[Cargo.toml](https://github.com/volcengine/OpenViking/blob/2562688c050e72fa1a6526732cec1139566677e9/crates/ragfs/Cargo.toml#L89)）；Helm 强制单副本，因 RocksDB 不支持多 Pod 共享（[deployment.yaml](https://github.com/volcengine/OpenViking/blob/2562688c050e72fa1a6526732cec1139566677e9/deploy/helm/openviking/templates/deployment.yaml#L1-L3)）；分布式为商业版 | 多一套单副本有状态服务，备份、恢复和一致性单独处理 |
| 主仓 AGPL-3.0；`crates/ov_cli`、`ragfs` 为 Apache-2.0 | 进入正式检索路径前需法务评审 |

### 2.2 OpenViking 记忆

| 核查结论 | 与本项目边界的冲突 |
| --- | --- |
| LLM 整理的 Markdown 存于 `viking://user/{uid}/memories/<type>`（profile、preferences、entities、events、cases 等）；会话提交时由 LLM 写、改、硬删，无审批钩子 | 违反“写入经审批并绑定精确版本” |
| `memory.extraction_enabled` 只在会话提交路径读取（[session.py](https://github.com/volcengine/OpenViking/blob/2562688c050e72fa1a6526732cec1139566677e9/openviking/session/session.py#L2693)、[L2717](https://github.com/volcengine/OpenViking/blob/2562688c050e72fa1a6526732cec1139566677e9/openviking/session/session.py#L2717)）；`POST /sessions/{id}/extract` 不检查它（[sessions.py](https://github.com/volcengine/OpenViking/blob/2562688c050e72fa1a6526732cec1139566677e9/openviking/server/routers/sessions.py#L704-L712)）；MCP `remember`、导入 `reason` 和资源删除也会触发记忆提交或解除引用 | 关闭开关不能保证不写记忆 |
| 记忆位于 user 空间（含 peers 子视图）；`is_accessible` 对 user 空间只放行本人或 ROOT，对 resources、agent 等空间不按用户限制（[namespace.py](https://github.com/volcengine/OpenViking/blob/2562688c050e72fa1a6526732cec1139566677e9/openviking/core/namespace.py#L317)） | 无客户或案件范围；叠加 trusted 模式，隔离取决于调用方 |
| 记忆不能存 PostgreSQL；使用它需把会话转录复制进 OpenViking | 与“JCode 会话以 PG 为唯一持久存储、不双写”冲突 |
| 有 `memory_diff.json`，回滚仅文档描述；LoCoMo 80–83% 为厂商自报（Doubao 评判），仓库内无法复现 | 缺少可验证的撤回与质量证据 |

rag-foundation 已要求不建 OpenViking session、不调用 `remember`、导入 `reason` 留空；若继续使用 OpenViking，这些约束保持不变。

### 2.3 Knowhere

Ontos-AI/knowhere `dc37bace`（2026-09-23），部署仓 knowhere-self-hosted `9e22e90d`。

| 核查结论 | 对本项目的影响 |
| --- | --- |
| 定位为 document memory：解析文档后提供检索与 Agent 式浏览；无用户或会话记忆，仓库未提及 OpenViking | 与 OpenViking 无直接关系，只在文档检索一半重叠；替代不了记忆 |
| 纯词法检索：应用侧 jieba 分词后写入 `to_tsvector('simple', …)`（[迁移](https://github.com/Ontos-AI/knowhere/blob/dc37bace0d96d7fcacf70917be117256b1104b26/apps/api/alembic/versions/e4f5a6b7c8d9_drop_chunk_search_tsv_columns.py#L40-L45)），正文与章节路径两通道按 RRF（k=60，[settings.py](https://github.com/Ontos-AI/knowhere/blob/dc37bace0d96d7fcacf70917be117256b1104b26/packages/shared-python/shared/services/retrieval/settings.py#L6)）融合；无 embedding/pgvector，rerank 开关未实现 | 精确料号和中文词可能较好，语义改写召回弱；应用侧分词写 `simple` 列可借鉴 |
| 范围为 namespace（自由字符串）+ include/exclude 文档 ID，在 SQL 候选阶段实施、hydration 再核对；MCP 语料工具上下文只有 user 与 namespace（[dynamic_tools.py](https://github.com/Ontos-AI/knowhere/blob/dc37bace0d96d7fcacf70917be117256b1104b26/apps/api/app/mcp/dynamic_tools.py#L50-L62)），REST `retrieval.query` 无 include 列表；`document_metadata` 仅展示 | 客户/料号/修订/有效期需外部换算为文档 ID；MCP 路径无法按请求收窄 |
| 对外引用字段为 document_id、source_file_name、section_path、page_nums（[row_utils.py](https://github.com/Ontos-AI/knowhere/blob/dc37bace0d96d7fcacf70917be117256b1104b26/packages/shared-python/shared/services/retrieval/hydration/row_utils.py#L10-L22)）；page_nums 只出现在 V2 页面块，内容为 VLM 摘要而非原文；V2 页面资产在 Worker 产物中带 `bbox_px`（[_serialization.py](https://github.com/Ontos-AI/knowhere/blob/dc37bace0d96d7fcacf70917be117256b1104b26/apps/worker/app/services/page_memory/_serialization.py#L158-L170)），但数据库模型、检索与引用返回均未使用；精确区域跳转为开放 issue [#223](https://github.com/Ontos-AI/knowhere/issues/223)；XLSX 无行列定位 | 达不到页/区域/单元格定位要求 |
| V1 PDF 默认 MinerU 云端 `https://mineru.net/api/v4`（[mineru.py](https://github.com/Ontos-AI/knowhere/blob/dc37bace0d96d7fcacf70917be117256b1104b26/packages/shared-python/shared/core/config/mineru.py#L11-L14)）；V2 PDF 走文本层 → RapidOCR（扫描件）→ 每页一次 VLM，按 144 dpi 渲染页面 PNG；图集（atlas）解析只让 VLM 从标题栏提取图集号、图集名称与页码标签（[prompt_service.py](https://github.com/Ontos-AI/knowhere/blob/dc37bace0d96d7fcacf70917be117256b1104b26/packages/shared-python/shared/services/ai/prompt_service.py#L848-L860)） | V2 页面路线可借鉴；图纸理解不足 |
| Agent Harness 默认 `cursor_sdk`（[resolve.py](https://github.com/Ontos-AI/knowhere/blob/dc37bace0d96d7fcacf70917be117256b1104b26/packages/shared-python/shared/services/retrieval/agent_explore/harness/resolve.py#L18)），工具结果会发往 Cursor 云；Webhook 依赖 QStash；PPTX 转换调用 iLoveAPI（[parser.py](https://github.com/Ontos-AI/knowhere/blob/dc37bace0d96d7fcacf70917be117256b1104b26/apps/worker/app/services/document_parser/formats/pptx/parser.py#L49-L54)）；PostHog 遥测默认开启（[base.py](https://github.com/Ontos-AI/knowhere/blob/dc37bace0d96d7fcacf70917be117256b1104b26/packages/shared-python/shared/core/config/base.py#L41-L47)），可用 `TELEMETRY_ENABLED=false` 关闭 | 每项都需显式关闭或替换，否则资料外发 |
| 只有软归档，API 唯一的 DELETE 路由是 webhook secret（[webhook_secrets.py](https://github.com/Ontos-AI/knowhere/blob/dc37bace0d96d7fcacf70917be117256b1104b26/apps/api/app/api/v1/routes/webhook_secrets.py#L160)），chunk、section 与 S3 对象保留；任务创建无幂等键，重复派发见开放 issue [#286](https://github.com/Ontos-AI/knowhere/issues/286) | 不满足撤回、重复上传与重启恢复要求 |
| API key 即拥有完整权限的用户；dashboard JWT 解码关闭 audience 校验（[`verify_aud: False`](https://github.com/Ontos-AI/knowhere/blob/dc37bace0d96d7fcacf70917be117256b1104b26/apps/api/app/services/auth/dashboard_jwt_authentication_service.py#L266)）；隔离依赖应用层 SQL，无 RLS；`/internal/s3-events` 未见鉴权依赖，SNS 路径不调用已定义的签名校验函数，缺少 SNS/OSS/MinIO 标识的请求按 direct S3 事件处理（[service.py](https://github.com/Ontos-AI/knowhere/blob/dc37bace0d96d7fcacf70917be117256b1104b26/apps/api/app/services/s3_events/service.py#L18-L36)、[event_handlers.py](https://github.com/Ontos-AI/knowhere/blob/dc37bace0d96d7fcacf70917be117256b1104b26/apps/api/app/services/s3_events/event_handlers.py#L21-L46)） | 只能作为内部组件，权限仍需我们实现 |
| 其[设计文档](https://github.com/Ontos-AI/knowhere/blob/dc37bace0d96d7fcacf70917be117256b1104b26/docs/design/retrieval-heart-vessel-trace-optimization-plan.md)记录的生产窗口：经典检索 P50 10.99 s、P90 25.46 s；单次 agentic 41.31 s、单次 v2 109.36 s | 自报且非本地复现，但量级已超出交互预算 |
| compose 为 postgres:15-alpine（无 pgvector）、redis:7-alpine、localstack:3.8 与含 API/Celery/Next.js 的应用镜像（[compose.yaml](https://github.com/Ontos-AI/knowhere-self-hosted/blob/9e22e90d5a06a54bb7b88aef1595c52c48cf1d10/compose.yaml#L3-L55)） | 引入即多运维一套 Python/Next.js 栈 |
| 2026-04-30 建仓；调研日 3,478 stars、9 位贡献者（头部作者约 65%）、74 个 release（至 v1.2.17）；Apache-2.0，但 Worker 固定 `pymupdf==1.27.2`、`pymupdf4llm==1.27.2.1`（AGPL/商业双许可，[pyproject.toml](https://github.com/Ontos-AI/knowhere/blob/dc37bace0d96d7fcacf70917be117256b1104b26/apps/worker/pyproject.toml#L24-L30)） | 项目年轻、贡献集中；部署 Worker 即引入 AGPL 组件 |

### 2.4 JCode 上游记忆与当前实现

- 上游记忆按项目哈希存为 JSON 文件（`crates/jcode-base/src/memory.rs`）；召回依赖外部 “Jev” 相关性服务；sidecar LLM 自动抽取，抽取上下文包含工具结果（`memory_prompt.rs`），资料中的注入文本可能被写成记忆；多处按空白切词（`jcode-memory-types/src/lib.rs`），对中文无效。
- 当前 [Bridge](../../../../services/runtime-bridge/src/main.rs) 以 `Registry::empty()`、`agent.set_memory_enabled(false)` 和覆盖系统提示三重关闭上游记忆；记忆关闭时 [`extract_session_memories`](../../../../external/jcode/crates/jcode-app-core/src/agent/turn_execution.rs) 返回 0。
- 当前代价：每轮把最多 50 条、每条 ≤2000 字符的个人记忆以 JSON 全量写入系统提示（[用例](../../../../services/api/src/objects/application/memory.rs)、Bridge）。最坏约 10 万字符，粗估英文约 3 万 token、中文约 6 万 token，未实测；任一记忆变化都会改变系统提示前缀。
- [`permission_mode`](../../../../services/api/src/permissions/domain.rs) 只把 `request_proposal_approval` 固定为 ask，`memory_save` 跟随会话配置的模式，“完全访问”下可能不逐条确认。
- 可沿用的上游做法：Jev 请求把查询与候选都声明为不可信数据，候选按位置而非 ID 标识（`memory_jev.rs`）；阈值限定在 0.8–1、最多 10 条（`memory_jev.rs`、`memory.rs`）；`focus_query_text` 去掉工具与提醒噪声并把最新用户消息放在最前；注入前复核会话/项目绑定并丢弃超过 120 s 的结果（`memory/pending.rs`）；召回作为单独消息注入而非改写系统提示（`jcode-app-core/src/agent.rs`）；条目保留 `trust` 与 `superseded_by`。

### 2.5 其他记忆框架（公开资料，未本地运行）

| 项目 | 可借鉴 | 不直接采用的原因 |
| --- | --- | --- |
| [mem0](https://github.com/mem0ai/mem0) | `infer=False` 不经 LLM 直接写入；服务端 pgvector；2026-04 起抽取改为只追加，LLM 不再 UPDATE/DELETE | PostHog 遥测默认开启；为少量记录多引入一个服务；写入前无审批 |
| [Graphiti](https://github.com/getzep/graphiti) / [Zep](https://github.com/getzep/zep) | 双时间模型（事实有效期与记录时间分开） | 依赖 Neo4j（GPL/商业）或 FalkorDB（SSPL），无 PG 后端；Zep 为闭源服务 |
| [Letta](https://github.com/letta-ai/letta) | 分层记忆块 | 模型直接改写长期记忆，与审批冲突；旧版 V1 服务端已停止维护 |
| [MemOS](https://github.com/MemTensor/MemOS) | 图后端可选 postgres/pgvector；按 cube 设 ACL | 默认部署为 Neo4j + Qdrant，偏好记忆需 Milvus；默认 Qwen/bge 模型；写入依赖大量 LLM 改写 |
| [Cognee](https://github.com/topoteretes/cognee) | [多用户权限](https://docs.cognee.ai/core-concepts/multi-user-mode/permissions-system/overview) | 生产级 PG 图存储为授权产品 |
| [LangGraph PostgresStore](https://reference.langchain.com/python/langgraph.store.postgres) | 显式 `put`、可选 pgvector 索引、TTL 后台清理，与本方案最接近 | Python/JS 组件；本项目直接用 Rust/PG 实现同一模式 |

共同点：未见在写入前提供审批门的框架；[OWASP Agentic Top 10](https://genai.owasp.org/2025/12/09/owasp-top-10-for-agentic-applications-the-benchmark-for-agentic-security-in-the-age-of-autonomous-ai/) 将记忆与上下文投毒列为 ASI06（另见[记忆攻击面](https://genai.owasp.org/2026/05/13/memory-is-a-feature-it-is-also-an-attack-surface/)）；LoCoMo 等基准存在争议（[审计](https://penfieldlabs.substack.com/p/we-audited-locomo-64-of-the-answer)称约 6.4% 标准答案有误，另见[对 mem0 结果的质疑](https://blog.getzep.com/lies-damn-lies-statistics-is-mem0-really-sota-in-agent-memory/)），且不测授权、泄露、来源或修订/有效期。本项目记忆需用自己的反例验收。

### 2.6 许可证

MinerU 为自定义 [MinerU Open Source License](https://github.com/opendatalab/MinerU/blob/2f078fc678dcc1d89e19ac31e826d86845f58a1e/LICENSE.md)：Apache-2.0 加附加条款，向第三方提供在线服务须显著标注 MinerU，超过 1 亿 MAU 或月收入 2000 万美元需商业许可，违反即自动终止；GitHub 识别为 NOASSERTION。OpenViking 主仓 AGPL-3.0；PyMuPDF（Knowhere Worker 依赖）AGPL/商业；[ParadeDB](https://github.com/paradedb/paradedb) AGPL；[pgvector](https://github.com/pgvector/pgvector) 为 PostgreSQL License；pg_trgm 为 PostgreSQL 自带扩展；[zhparser](https://github.com/amutu/zhparser) 许可宽松；[pg_jieba](https://github.com/jaiminpan/pg_jieba) 维护停滞。

### 2.7 LLM Wiki（编译式知识库）

这里的“Wiki”指 LLM 把原始资料持续编译成互相链接的 Markdown 页面，而不是每次提问时临时检索。现有企业 Wiki（飞书、Confluence 等）只作为又一类文档来源，走同一解析与发布流程；OpenViking 已有飞书 Wiki 递归导入（[feishu_accessor.py](https://github.com/volcengine/OpenViking/blob/2562688c050e72fa1a6526732cec1139566677e9/openviking/parse/accessors/feishu_accessor.py#L475)、[feishu_import.py](https://github.com/volcengine/OpenViking/blob/2562688c050e72fa1a6526732cec1139566677e9/openviking/parse/feishu_import.py#L13-L17)），本节不讨论。

**模式**（[Karpathy gist](https://gist.github.com/karpathy/442a6bf555914893e9891c11519de94f)，2026-04-04）：自述为 “A pattern for building personal knowledge bases using LLMs” 的 idea file。它批评 RAG “rediscovering knowledge from scratch on every question. There's no accumulation”。结构分三层：不可变的原始资料（“your source of truth”）；LLM 生成、“The LLM owns this layer entirely” 的 Wiki；以及 CLAUDE.md/AGENTS.md 式的 schema。操作有 ingest（“A single source might touch 10-15 wiki pages”）、带引用的 query（好的回答可回存为新页面）和 lint（矛盾、过时、孤立页）；`index.md` 为目录，`log.md` 只追加。规模上称 “works surprisingly well at moderate scale (~100 sources, ~hundreds of pages)”，更大时需要真正的搜索（文中举 qmd：BM25/向量混合加 LLM 重排）。人工审核只写 “Possibly with humans in the loop reviewing updates”，没有事实校验方法。

**OpenViking `ov compile`**（同一 commit `2562688c`）已内置此模式：

| 核查结论 | 对本项目的影响 |
| --- | --- |
| 依赖 VikingBot（服务须 `--with-bot`），任务异步执行，VikingBot 以调用者身份在独立 Agent Loop 中读源、写页（[概览](https://github.com/volcengine/OpenViking/blob/2562688c050e72fa1a6526732cec1139566677e9/docs/zh/context-compilation/01-overview.md#L15)、[前置条件](https://github.com/volcengine/OpenViking/blob/2562688c050e72fa1a6526732cec1139566677e9/docs/zh/context-compilation/01-overview.md#L46)、[README](https://github.com/volcengine/OpenViking/blob/2562688c050e72fa1a6526732cec1139566677e9/README.md#L61)）；目标可以是 resource、memory 或 skills 目录（[compile_service.py](https://github.com/volcengine/OpenViking/blob/2562688c050e72fa1a6526732cec1139566677e9/openviking/service/compile_service.py#L464-L484)） | 又多一个 Agent 服务；compile 也是一条绕过审批的记忆写入路径 |
| 示例 [llm-wiki Skill](https://github.com/volcengine/OpenViking/blob/2562688c050e72fa1a6526732cec1139566677e9/examples/compile/ov-compile-skills/llm-wiki/SKILL.md) 规定 entity/concept/method/comparison/analysis/summary 页型，要求来源只读（L22）、资料内指令视为数据（L64）、只用给定来源（L66）、在论断旁放来源 URI（L220）、不编造 URI/数字/日期（L233）、时效性论断注明时期或版本（L247） | 规则设计可借鉴；但全部是提示词约束，没有代码校验 |
| 工具为 OpenViking 只读工具加 `read_file`/`write_file`/`edit_file`（[service.py](https://github.com/volcengine/OpenViking/blob/2562688c050e72fa1a6526732cec1139566677e9/bot/vikingbot/compile/service.py#L71-L81)）；direct 后端下 exec 取 `allow_compile_exec`，默认 `True`（[service.py](https://github.com/volcengine/OpenViking/blob/2562688c050e72fa1a6526732cec1139566677e9/bot/vikingbot/compile/service.py#L531-L543)、[schema.py](https://github.com/volcengine/OpenViking/blob/2562688c050e72fa1a6526732cec1139566677e9/bot/vikingbot/config/schema.py#L757-L761)，`restrict_to_workspace` 默认 `False`）；设计文档称 direct 只提供任务 cwd、非 OS 级隔离，多用户部署须关闭 | 与“产品 Agent 不开放通用 Shell”冲突，需显式关闭或换隔离后端 |
| 页面须引用有效 `source_ids`（[renderer.py](https://github.com/volcengine/OpenViking/blob/2562688c050e72fa1a6526732cec1139566677e9/bot/vikingbot/compile/renderer.py#L479-L483)）；正文未链接来源时自动追加“来源”列表，指向来源**目录** URI（[renderer.py](https://github.com/volcengine/OpenViking/blob/2562688c050e72fa1a6526732cec1139566677e9/bot/vikingbot/compile/renderer.py#L336-L362)） | 引用粒度是目录/文件，达不到页/区域/单元格 |
| 结果经 `batch_write(..., wait=False)` 直接写入目标（[service.py](https://github.com/volcengine/OpenViking/blob/2562688c050e72fa1a6526732cec1139566677e9/bot/vikingbot/compile/service.py#L1151-L1156)），未见预览或审批；写入模式只有 replace/append/create/upsert，无删除（[content.py](https://github.com/volcengine/OpenViking/blob/2562688c050e72fa1a6526732cec1139566677e9/openviking/server/routers/content.py#L56-L62)），设计文档说明批量写非跨文件原子；资源目标在迭代超限时仍把工作区文件 upsert 进目标，并警告 “This partial output did not pass the normal bundle validation”（[service.py](https://github.com/volcengine/OpenViking/blob/2562688c050e72fa1a6526732cec1139566677e9/bot/vikingbot/compile/service.py#L1031-L1045)、[L1500-L1520](https://github.com/volcengine/OpenViking/blob/2562688c050e72fa1a6526732cec1139566677e9/bot/vikingbot/compile/service.py#L1500-L1520)） | 未审批、可能未校验的内容直接可检索；来源撤回后旧页不会随之删除 |

**实证**（预印本，只读摘要，未复现）：

- [Cochran，arXiv 2605.18490](https://arxiv.org/abs/2605.18490)（v2 2026-08-16，预注册）：24 篇论文、13 题、同一回答模型。Wiki 构建成本高约两个数量级，每题 token 约为 RAG 的 21 倍，“no break-even point exists”；分解检索式 RAG 以更低 token 成本拿回 Wiki 几乎全部综合优势；Wiki 保留逐条论断的引用支持优势。整体 groundedness 与逐条引用核查几乎不相关（rho = 0.04），评测方法本身影响结论。
- [Huerta 等 WiCER，arXiv 2605.07068](https://arxiv.org/abs/2605.07068)（2026-05-08）：盲编译 “fails entirely”（2.14–2.32 对 3.46，53–60% 灾难性失败），原因是丢失关键事实；1–2 轮针对性诊断修补恢复 80% 损失，灾难性失败相对减少 55%，针对性诊断 +0.95 远高于泛泛置顶 +0.16。精选知识全量放入上下文优于 RAG（4.38 对 4.08，TTFT 快 7.3 倍），但规模增大后因注意力稀释低于 RAG。

此外，MinerU Document Explorer 也有 Wiki 写入、来源字段和断链检查，见 [rag-references](rag-references.md) 第 2 节。

**与本项目边界的冲突**：

1. **事实权威**：Wiki 页是 LLM 的派生物，数量、修订、尺寸必须回到原件块或 ERP。
2. **修订与有效期**：“LLM 全权维护”的页面会把新旧修订合并成一段叙述；本项目由后端按发布版本与有效期裁决，页面无法表达“对本案件哪一版有效”。
3. **授权范围**：一页综合多份资料时，可见性取所有来源的交集；跨客户页一旦生成即成为泄露载体，检索前过滤也失效。
4. **撤回传播**：来源撤回或被取代后，所有引用它的页面须失效；上游无删除写入模式，也无依赖追踪。
5. **写入审批**：业务知识沉淀需要授权人批准精确版本；`ov compile` 直接写入。
6. **投毒**：资料内指令经编译被固化进页面并反复召回，即 OWASP ASI06；只靠 Skill 提示词不足以防范。

## 3. 需求 × 能力

| 需求 | OpenViking（资源） | Knowhere | PG 原生（自建） |
| --- | --- | --- | --- |
| 召回前按客户/项目/料号/修订/有效期限定 | 路径前缀 + tag 等值；范围条件需预先换算 | namespace + 文档 ID；MCP 不能按请求收窄 | 同一 SQL JOIN 授权与发布表，支持范围条件，可加 RLS |
| 精确料号/图号 | 本地默认只有稠密向量 | BM25 + jieba | 规范化标识列精确匹配，trigram 兜底 OCR 误差 |
| 语义召回 | 稠密向量 + 分层摘要（主要优势） | 无 | pgvector；分层摘要需自建（可选） |
| 页/区域/单元格引用 | 命中不带位置，需自建映射 | 章节路径与页码 | 解析产物直接入库，检索返回块 ID |
| 两类图片 | 图片语义 Experimental，大图不存原件 | V2 页面路线；图纸浅 | 取决于共用解析层 |
| XLSX 单元格 | GFM 表格，1000 行截断 | 无行列定位 | calamine 保留 |
| 发布、撤回、回滚 | 非原子删除；需独立路径 + PG 指针 + 对账 | 仅软归档 | 同一事务切换指针，可硬删 |
| 入库幂等与恢复 | 自有持久队列，需与 PG 对账 | 无幂等键 | 沿用 PG 任务/租约 |
| 云依赖与遥测 | 可全部自托管 | 多项默认云端，遥测默认开 | 无 |
| 身份与授权 | 单密钥可冒充，必须由后端代理 | API key 全权，无 RLS | 与现有身份和 EBAC 一致 |
| 运维增量 | 单副本有状态服务 | Python/Next.js + Redis + LocalStack + PG15 | 现有 PG 加 pgvector |
| 许可 | AGPL-3.0 | Apache-2.0，含 AGPL 依赖 | PostgreSQL License |
| 延迟 | 未实测 | 自报经典检索 P50 约 11 s | 未实测 |
| 长期记忆 | 有，但与边界冲突 | 无 | 按第 5.1 节自建 |

## 4. 方案组合

| 方案 | 优点 | 代价与风险 |
| --- | --- | --- |
| A. 保持现方向：自有解析 → OpenViking 检索 | 沿用已确认决策与现有适配；分层检索可能改善长文召回 | 业务过滤编码为路径/URI 集合；精确标识检索另做；双存储一致性与撤回对账；AGPL 单副本服务；需主动关闭其记忆与 MCP |
| B. 自有解析 → PG 原生混合检索（推荐默认） | 范围、版本、发布、引用、授权同在一个事务存储；不增加服务；即 rag-foundation 第 4 步本来要做的基线 | 召回、融合与可选摘要层需自行实现调优；引入 pgvector 并验证带过滤召回；中文分词需实测 |
| C. Knowhere 端到端替换 | 自带解析、词法检索、浏览工具与控制台 | 引用、删除、幂等、云默认、性能与许可缺口需逐项修补，工作量接近自建，还多一整套技术栈 |
| D. Knowhere 只做解析，接 A 或 B | 复用其 V2 页面处理 | 区域坐标不进入持久模型，依赖 PyMuPDF；是否优于直接使用 MinerU 未知，仅适合作解析对照 |

解析规范化、评测集与引用映射是 A、B 共用的工作，两者差别主要在检索层。先做 B，它同时就是 A 的对照基线；评测后即使改选 A，前序工作也不浪费。

## 5. 推荐

### 5.1 长期记忆（与 RAG 选型无关）

1. **数据**：新增时间戳迁移扩展 `user_memories`：`scope_kind`（user/customer）与 `scope_id`、`category`（preference/fact/procedure/correction）、`status`（proposed/active/superseded/rejected/retracted）、`supersedes`、`valid_from`/`valid_to`、`review_after`、`source`（会话、消息、案件、document_version_ids、tool_call_id）、批准绑定（批准人、时间、文本哈希或版本）、`updated_at`，可选 embedding。现有行迁移为 user/active。只追加：编辑即新版本并使原批准失效；审计沿用现有事务审计。
2. **写入**：模型只能提议，沿用 `memory_save` 挂起审批。个人记忆由本人确认；客户记忆由对该客户有权限的授权角色批准，绑定精确文本版本。模型不删除、不改写，由用户在界面停用或撤回。
3. **召回**：后端每轮从身份与当前案件推导范围，不接受模型传入的范围。少量置顶项放在稳定的系统提示；其余按最新用户消息做词法（可选向量）召回，限定条数与 token 预算（初值建议约 1500，按评测调整），作为本轮尾部不可信上下文注入，并记录注入的记忆 ID。系统提示前缀因此保持稳定，缓存收益需实测。
4. **用途边界**：记忆只作分析提示，不作为数量、版本或 ERP 状态的依据，涉及时必须回到资料或 ERP 核对。组织级流程写进 Skill，案件事实留在案件上下文。
5. **不自动抽取**：不从工具结果或资料中抽取记忆，避免资料内指令借此持久化。后续如需建议功能，只由离线 Worker 从用户本人消息生成 proposed 候选。

### 5.2 RAG：PG 原生骨干

1. **解析**（与 OpenViking 臂共用）：自托管 MinerU，保留 content_list/middle_json 的页码与坐标（坐标系以锁定版本实测为准）；XLSX 保留工作表、单元格、公式原值与显示值；照片/图纸由视觉模型生成结构化描述，歧义进入 needs_review；扫描页借鉴 Knowhere V2 的“文本层 → 本地 OCR → 逐页 VLM”并保存页面预览。
2. **数据**：块表保存 document_version、block、父子关系、原文或结构化表格与页/区域/单元格定位；解析阶段抽取的客户、料号、图号、修订写入规范化列；发布指针与有效期在 PG。
3. **检索**：同一 SQL 先按租户、授权、已发布版本与有效期收窄候选，再走 pgvector、中文全文（应用侧分词写入 `simple` tsvector，备选 zhparser）、标识精确匹配 + trigram 三个通道，按 RRF 融合。带过滤的向量检索需验证不丢召回（pgvector 迭代索引扫描，或小范围内精确扫描）；缓存键包含范围，hydration 复核权限与发布状态。
4. **工具**：保持 `search_case_knowledge(query)` 与 `read_knowledge_source(source_id)`；读取逐步支持目录、邻近块和表格区域，由 JCode 自行多轮深读。适配层接口不变，后端换成 OpenViking 也不影响工具 schema。
5. **OpenViking 对照臂**：导入同一规范化产物，使用同一 embedding，每个修订独立路径；只经后端 REST 访问，网络上不暴露 `/mcp`；不建 session、不调用 remember、不传 reason。

### 5.3 改变选择的条件

- **OpenViking**：评测中明显优于 PG 基线（尤其长文与多层结构题），隔离反例全部通过，延迟与入库成本可接受，且 AGPL 评审通过时，作为检索引擎接入同一适配层；PG 继续负责边界与引用映射。
- **Knowhere**：补齐区域引用（#223）、向量或混合召回、硬删除、元数据过滤、MCP/REST 按请求限定范围和任务幂等，改为不默认依赖云，并解决 PyMuPDF 许可后可重新评估。
- **PG 方案**：混合负载下的延迟或规模不达标时，再评估专用检索引擎，仍接在适配层后面。
- **LLM 自动编译 Wiki**：R0+ 在跨文档综合题上仍系统性失败，W 臂通过关键事实保留与隔离反例，且构建与每题成本可接受时，才按第 5.4 节第 5 项试点。

### 5.4 Wiki：视图与导航，不是事实来源

1. **确定性目录**（第一阶段，替代 `index.md`）：由 PG 元数据生成“客户 → 项目 → 料号 → 修订/有效期 → 文档”树，只含当前用户可见的已发布版本。可选逐文档摘要标为派生物，随版本失效。作为 `read_knowledge_source` 的目录模式提供，不新增服务或工具。
2. **沉淀落在记忆条目**：客户经验、纠错与约定以第 5.1 节的审批记忆条目保存（需范围 B）；组织级流程写成 Skill。
3. **知识页是渲染视图**：客户/料号知识页 = 已批准条目 + 资料目录 + 修订历史，由后端按当前权限实时渲染，不单独存储。页上每条内容链接到原件块或记忆条目。
4. **不进 Wiki**：ERP 实时状态始终走 ERP 工具；客户料号映射等主数据属于 ERP 主数据，不在页面中另存一份。
5. **LLM 起草页面**（满足第 5.3 节条件后）：离线 Worker、单一授权范围、只读受限工具，不给 Shell、ERP 或写入工具；产出为待审页面版本，逐条链接块 ID；按 WiCER 思路核对关键字段（料号、数量、日期、修订）是否保留；授权人批准后发布；引用的版本被取代或撤回时页面自动标为过期并停止注入。可复用 llm-wiki Skill 的页型与引用规则，但不部署 VikingBot 或 `ov compile`。

要替换的是“切块 + 单次 top-k + 塞进提示词”的朴素 RAG，替换方式是范围优先的混合检索、Agent 多轮深读原件，加上确定性目录导航。Wiki 的价值在沉淀与导航，不在取证。

## 6. 验证计划

### 6.1 检索对照

- 数据：rag-foundation 约 40 题中文合成集，覆盖两客户、相近料号、新旧修订、表格截图、扫描件、照片、图纸及模糊/冲突/无答案题。
- 解析单独评测：直接使用 MinerU 与可选替代的关键字段准确率和定位成功率。
- 检索臂使用同一解析产物、embedding 与回答模型，先不开重排：R0 为 PG 单次检索；R0+ 为 R0 加深读工具；R1 为带 `target_uri` 与完整分层处理的 OpenViking `find`，另记录 `vectors_only`；可选 R2 为 Knowhere 经典检索。
- 跨文档综合题：另加一组需要汇总多份资料的问题（如某客户多次修订的累计变化）。先比较 R0 与 R0+；只有 R0+ 仍系统性失败时才加 W 臂（按第 5.4 节第 5 项离线编译的页面），并记录关键事实保留率、构建成本、每题 token，以及修订后过期页面是否被阻断。
- 指标：证据 Recall@5、引用正确率与定位成功率、拒答正确性、隔离与版本反例（必须 100%）、关键字段准确率、端到端与工具延迟 p50/p95、入库耗时与成本、运维组件数。
- 判定：任何隔离失败直接淘汰该臂；其余优先简单方案（PG < OpenViking < Knowhere），除非复杂方案有明确、可解释的收益。40 题下相差 3–5 题以内难与噪声区分，应看逐题差异与错因，必要时扩充题量并做配对检验；不预设百分比。

### 6.2 记忆

- 跨用户、跨客户泄露为 0，覆盖召回候选与注入内容；被取代或撤回的记忆不再注入。
- 约 20 条查询的召回精度；注入量不超出 token 预算。
- 编辑已批准记忆会使批准失效；记忆中的指令文本不改变工具权限或审批状态。
- API/Worker 重启后，proposed 状态与审批可恢复。

## 7. 风险与待确认

1. **默认检索骨干改为 PG、OpenViking 降为对照臂，需用户确认。** 确认后需同步修改 [AGENTS.md](../../../../AGENTS.md) 的 Accepted Direction、[决策](decisions.md)第 5/12/26 条、[Rust 后端与上下文](rust-auth-context.md)第 7 条、[rag-foundation](rag-foundation.md) 的服务分工，以及决策 14 中工具名称所指的 OpenViking。本文未修改这些文件。
2. **记忆范围**：A 仅个人；B 个人 + 客户共享。建议以 B 为目标，表结构预留、先上线 A；客户范围授权（EBAC 资源范围或 ERP Customer 绑定）就绪后再开放 B。
3. **`memory_save` 与“完全访问”**：建议客户范围始终审批；个人范围是否固定为 ask 由用户决定；若允许自动保存，记录当时模式，默认不置顶，并可一键撤回。
4. **许可**：OpenViking（AGPL）、MinerU 附加条款、PyMuPDF（若引入 Knowhere）均需法务评审后才能进入生产路径。
5. **技术待验**：本地 PG 镜像 [`postgres:17.6-bookworm`](../../../../infra/compose.yaml) 不含 pgvector，需换镜像或安装扩展；带过滤的向量召回；中文分词；embedding 模型未选定（见[待确认信息](open-questions.md)）；MinerU 锁定版本的坐标输出；视觉模型对图纸的效果。
6. **证据性质**：本文事实来自固定 commit 的源码与文档阅读，外部框架来自公开资料；预印本结论只读摘要，未复现；均未本地部署或运行，不构成性能或集成验收。
7. **Wiki 定位**：建议只做确定性目录与记忆渲染视图（第 5.4 节）；不部署 VikingBot/`ov compile`；LLM 不直接发布页面。客户知识页依赖记忆范围 B。需用户确认。

## 8. 版本与来源

| 对象 | 固定版本 |
| --- | --- |
| OpenViking（含 `ov compile`/VikingBot） | [`2562688c050e72fa1a6526732cec1139566677e9`](https://github.com/volcengine/OpenViking/tree/2562688c050e72fa1a6526732cec1139566677e9)（2026-09-24） |
| Knowhere | [`dc37bace0d96d7fcacf70917be117256b1104b26`](https://github.com/Ontos-AI/knowhere/tree/dc37bace0d96d7fcacf70917be117256b1104b26)（2026-09-23） |
| knowhere-self-hosted | [`9e22e90d5a06a54bb7b88aef1595c52c48cf1d10`](https://github.com/Ontos-AI/knowhere-self-hosted/tree/9e22e90d5a06a54bb7b88aef1595c52c48cf1d10) |
| JCode 子模块 | `6819cf87acdc6cc4e2c2c321148eb59c04d82749` |
| MinerU LICENSE.md | [`2f078fc678dcc1d89e19ac31e826d86845f58a1e`](https://github.com/opendatalab/MinerU/blob/2f078fc678dcc1d89e19ac31e826d86845f58a1e/LICENSE.md) |
| Karpathy LLM Wiki gist | [442a6bf5…](https://gist.github.com/karpathy/442a6bf555914893e9891c11519de94f)，创建于 2026-04-04，调研日读取，可能被修订 |
| Cochran 预印本 | [arXiv 2605.18490v2](https://arxiv.org/abs/2605.18490v2)（2026-08-16） |
| WiCER 预印本 | [arXiv 2605.07068v1](https://arxiv.org/abs/2605.07068v1)（2026-05-08） |

外部框架与 Wiki 资料链接见第 2.5–2.7 节，均为未本地验证的公开资料。
