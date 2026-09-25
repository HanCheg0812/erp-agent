# OpenViking RAG 接入方案

日期：2026-09-22。状态：分批实施；Skill 与 Rust 解析库已落地，真实 RAG 链路尚未实现或部署。

用户确认：RAG 效果是项目核心；首批图片同时包含需求单、表格截图、扫描件，以及产品照片、零件图或工程图纸。本文记录接入建议和验证顺序，未把候选模型、接口及指标写成已验收能力。

相关：[原始业务架构](../../../architecture/erp-agent-demo-architecture.md)、[业务 Prompt 草案](../../../../prompts/erp-agent-system-prompt.md)、[已确认决策](decisions.md)、[验收基线](../../baseline/test-and-release-gates.md)。

2026-09-22 补充：[开源实践调研](rag-references.md)。借鉴 MinerU 官方 Skill 的 locator 续读、Document Explorer 的检索后深读与图表读取、OpenViking 社区工具循环；具体适配仍需探针验证。

用户进一步确认 Skill 引导与 Rust 优先，见 [Skill/工具设计](skill-tool-design.md)；多解析器配置继承、父子产物与模型比较见 [解析流水线设计](parser-pipeline-design.md)。聊天附件绑定、JCode 图片输入与知识路径见 [附件设计](chat-attachment-agent-design.md)。首批实现和边界见 [联调记录](skill-parser-integration.md)，以下完整 RAG 职责与接口仍是目标设计。

2026-09-24 补充：[知识检索与长期记忆方案调研](knowledge-memory-research.md)比较 OpenViking、Knowhere 与 PostgreSQL 原生混合检索，并给出长期记忆升级方案及 LLM Wiki 的定位。随后形成的[记忆与 OpenViking 架构](memory-openviking-architecture.md)确定云端边界：PG 负责记忆权威状态与 pgvector，OpenViking 统一负责企业 Resource/Wiki/RAG；PG 资料检索保留为可复现实验基线，不能把实验基线写成已替换 OpenViking 的生产决定。

## 目标与当前缺口

原始业务主线是客户需求变更分析：读取需求资料，结合 ERP 当前事实，给出有依据的影响分析、方案与人工审批。RAG 负责把适用的资料准确交给 Agent，尤其要区分客户、项目、料号、技术修订、需求修订、生效时间，以及增量/累计/替换总量口径。

当前 JCode/DeepSeek 对话循环、Skill 发现/加载和 PostgreSQL 会话恢复已经验证；RustFS 原件上传和授权下载已实现，但聊天消息尚未绑定附件，JCode Bridge 仍只收到文字。Rust 解析库具备配置继承与可替换后端契约，真实解析任务、OpenViking 检索和 MinerU 尚未接入。Web 演示中的知识引用是合成资料，真实侧栏目前只有保存的引用字段。

下一阶段先验证两类图片的上传、解析、问答与原文定位。不以“服务启动成功”代表 RAG 效果提升，也不在本阶段加入 CAD 编辑、几何测量、精细排产或自动 ERP 写入。

## 服务分工

| 所属模块 | 职责 |
| --- | --- |
| `apps/web` | 聊天附件上传、缩略图、处理状态、引用和原文定位；沿用灰白 Chat 与证据侧栏 |
| `services/api` 文档领域 | 身份/案件权限、原件登记、版本与有效期、解析任务、发布状态、引用读取 |
| PostgreSQL | 文档/版本/来源映射、任务租约与步骤、发布指针、审计；继续独占 JCode 会话持久化 |
| 原件及产物存储 | 持久保存原始字节、解析 JSON、页面/图块、缩略图；独立于 Cube，索引可由它重建 |
| `services/ingestion-worker` | 异步解析、校验、调用视觉模型、导入索引、确认完成；共享模型服务在会话沙箱外 |
| MinerU / 结构化解析器 | 文档 OCR、版面、表格；Excel 直接保留工作表、行列、公式原值与显示值 |
| 视觉模型 | 产品/零件图片语义和图纸区域解释，输出可核对的来源与不确定项 |
| OpenViking | 资源组织、分层摘要、向量索引与检索；不接管业务版本裁决或 JCode 会话 |
| `services/mcp-gateway` / JCode | 将授权范围内的知识搜索、原文读取提供给现有 Agent Loop |

首个本地 Demo 可使用专用持久卷存原件，并复用 API 已有 PostgreSQL 队列模式建立独立文档任务表；不要仅为首轮接入再增加一套队列服务。若后续引入 Redis Streams，由 PostgreSQL 任务/outbox 驱动，投递语义仍为至少一次。

## 入库流程

```text
Chat 附件上传
  -> 业务 API 校验文件、保存原件、登记版本与持久任务
  -> Worker 按内容路由：OCR/表格 + 必要的视觉解析
  -> 保存带来源坐标的统一解析产物
  -> 校验关键字段、版本归属与处理完整性
  -> 导入 OpenViking 的独立版本索引，等待语义/向量任务完成
  -> PostgreSQL 发布该可检索版本，Web 显示可使用
```

建议初期接收 PNG/JPEG、PDF 和 XLSX；更多格式按实测扩展。文件类型、大小、像素/页数限制在上传契约中明确，先验证实际字节再解码。图片是正式附件，支持选择、拖放和粘贴；不把图片 base64 塞进普通聊天正文。

上传返回文档 ID、版本 ID 和任务 ID；业务请求不等待 OCR。状态区分 `queued / parsing / validating / indexing / ready / needs_review / failed / cancelled`。用户可查看进度、失败原因和重试；解析未完成时，Agent 不把附件当作已读资料。

任务幂等键至少包含租户、文件哈希、解析配置版本和目标文档版本；相同字节不跨授权范围自动复用。租约、心跳、有限重试和步骤检查点沿用现有队列实践。OpenViking 调用超时后先核对已记录的任务/目标，避免重复导入。

取消或重试后的过期 Worker 不得发布结果。原件先可靠落盘，再事务登记可处理任务；中断造成的未登记文件由清理任务回收。部分文件解析失败或向量任务未完成，不把整个版本标记为 ready。新版本在隔离位置构建，失败时既有已发布版本仍可读，但不能因它可读就认定它对当前问题仍然有效。

## 两类图片的解析

| 输入 | 解析与保留内容 | 关键校验 |
| --- | --- | --- |
| 需求单、扫描 PDF、表格截图 | OCR、版面、完整表头与行列、单位、页面及坐标；必要时视觉模型补充 | 料号、数量、日期、修订、合并单元格和总量口径 |
| 产品照片、零件图片 | 原图、可见文字、结构化视觉描述、相关区域；关联产品/料号待业务确认 | 不凭外观相似自动确认同一料号，不从照片比例推算尺寸 |
| 工程图纸 | 页面/大图分区、标题栏、标注文字、尺寸/公差/单位、图块与关系说明 | 数值、符号和归属存在歧义时进入 needs_review，不把视觉猜测当工程事实 |

这些类型可以在同一页出现，路由不是互斥分类。保留 OCR 结果与视觉解释两种产物，不以图片描述覆盖原文。关键字段记录提取方式、解析器/模型版本和校验结果；无可校准置信度时使用“待核对”，不伪造可信度分数。

统一产物至少包含：`document_id`、`revision_id`、`source_hash`、`block_id`、块类型、正文/表格结构、页码或工作表单元格、原图坐标与坐标系、父子关系、解析配置版本。缩放、旋转、切片保留到原图的变换映射；无法定位的结果不得伪造高亮框。

MinerU 的具体图片接口、图纸符号表现及坐标输出需要用锁定版本实测。已核实的 OpenViking 内置 MinerU 适配是 PDF `/file_parse`，不能据此推断所有图片都走同一路径。为保留结构化产物和定位信息，建议 Worker 先解析，再将规范化文本、表格和关联图片导入 OpenViking，避免重复 OCR 和丢失位置。

新核查的 MinerU 官方 Skill 面向 4.x，提供 `parse/read`、质量档位和页/块 locator，不能假定与上述 `/file_parse` 适配直接兼容。先验证选定版本的接口；把原生 locator、解析配置及产物绑定到业务文档版本。缓存 ID 和块号不能脱离原件哈希与解析版本充当永久引用。

当前专用 DeepSeek 配置已验证文字对话；官方 [图像理解文档](https://api-docs.deepseek.com/zh-cn/guides/vision)说明 Flash 支持 Responses 图片输入，固定 JCode 版本已有 `ContentBlock::Image`，但 Forge 尚未把附件送进回合，也未做真实视觉探针。向量模型尚未配置；不继承开发者登录。上传图片供文字问答与“以图搜图”是不同能力：前者可先用即时看图或 OCR/视觉描述配合文本向量，后者需要多模态向量，并通过对照评测决定是否进入首轮。

## 检索与 Agent Loop

```text
用户问题 + 当前案件
  -> JCode 调用知识工具
  -> 后端从身份/案件确定允许的客户、项目、料号、修订与有效期
  -> OpenViking 限定范围召回，按需重排和读取详情
  -> 后端复核权限、发布状态、版本和来源映射
  -> Agent 使用证据回答，必要时继续检索或请求澄清
  -> 点击引用，由业务 API 打开原文件的页面/区域/单元格
```

沿用已确认的 JCode -> 受控 stdio MCP -> 知识适配 -> OpenViking HTTP MCP 链路。上传及批量索引由 API/Worker 调用内部接口，不交给模型循环执行。

候选工具保持少量：`search_case_knowledge(query)` 与 `read_knowledge_source(source_id)`。身份及案件授权由后台注入；工具入参不能任意改租户或 URI。前端展示“搜索资料”“读取原文”和 OpenViking 来源，不加 MCP 后缀。实际结果回到 JCode 后再决定下一步，不预设固定两次调用。

参考 Document Explorer，读取工具可逐步支持目录、正文块、表格/图块和有界邻近上下文；具体参数待兼容探针确定。长文沿返回的定位继续读，数字结论读取对应原文。官方 CLI Skill 的阅读策略可以转成业务 Skill，但不向产品 Agent 开放任意 Bash 或文件路径，也不因参考 QMD 示例就增加第二套正式索引。

后端按 PostgreSQL 已发布且适用的版本集合构造 `target_uri` / 过滤条件，不能全库查询后仅靠模型筛选。查询与 read 均复核权限，失效/撤回版本及时阻断；隔离范围也覆盖分层摘要和重排输入。不能只做最终 top-k 后过滤而掩盖召回损失。

首轮使用能限定 `target_uri` 的 `find` 或 `search` list 模式。多轮问题所需的案件信息和指代由现有 PostgreSQL 会话生成有界查询，不另建 OpenViking session，也不调用 `remember`/会话提交。导入 `reason` 留空，避免该上游参数隐式生成长期记忆。

L0/L1 摘要用于定位，关键业务结论引用原件对应的 L2 内容和解析块。表格按完整语义行/表头组织，图纸块带上标题栏和单位等父级上下文。精确料号/图号匹配与语义召回结合，先测上游支持的检索方式；是否追加关键词分支和重排由错例决定，不预先增加第二套检索引擎。

每个证据返回版本、原文片段、来源类型和稳定定位；原图可由前端查看。若回答必须重新辨认视觉细节，通过后端受控视觉解析复核具体区域，不假定当前 JCode/DeepSeek 已能消费 MCP image 内容。资料中的指令作为不可信内容处理，不改变工具权限。

## 首轮实施顺序与验收

1. 建立合成评测集与解析探针：两客户、相近料号、新旧需求/技术修订、表格截图、扫描 PDF、产品照片和图纸；保留模糊/冲突/无答案样本。锁定 OpenViking、MinerU、视觉及向量模型版本。
2. 打通上传、原件存储、任务恢复及证据预览。解析产物先能核对原件；关键字段冲突可见，重复上传和 Worker 重启不产生重复发布。
3. 接入 OpenViking 和受控知识工具，完成每类图片至少一个“上传 -> 解析 -> 提问 -> 正确引用 -> 点击原文”的真实案例。API/Worker 重启后原件、任务、索引映射和对话仍可恢复。
4. 固定评测集对比基础分块向量检索与 OpenViking 分层检索，再单独比较 OCR/视觉补充、多模态向量及重排收益。基础对比使用相同解析产物、embedding 和回答模型，记录差异，避免把换模型的收益算成检索收益。
5. 依据错例修正解析、检索或引用，之后再与 ERP 当前事实合并验收；ERP 库存/订单实时状态仍来自 ERP 工具。

首轮建议约 40 个有标注问题，按扫描/表格、照片/图纸、版本冲突、无答案分别统计。报告关键字段准确率、证据 Recall@5、引用正确率及定位成功率、应拒答时的表现、端到端延迟、解析耗时和调用成本；不能只报回答流畅度或平均延迟。

固定隔离与版本反例必须全部通过：不暴露其他客户资料；未发布资料不作正式依据；历史版本仅在明确的历史查询中作为历史材料使用；依据不足时不生成确定数量/尺寸。关键数字引用需能回到对应区域。RAG 质量提升需相对固定基线有逐题记录，不预先承诺百分比。

新索引/解析版本通过上述测试后再切换；失败可回退发布指针或关闭知识工具，原件和会话不受影响。撤回的业务版本不能因技术回滚重新生效。批量解析并发与在线检索分别限流，复用沙箱外的共享模型服务，后续再测混合负载。

## 上游核查证据与待验证项

本轮查阅 OpenViking HEAD `172c1050716f63339f4001292d58c830e6b7a315` 的文档和源代码；这是研究快照，不代表已选部署版本或本地验收。

| 核查结论 | 来源及对本项目的影响 |
| --- | --- |
| PDF 支持 local/mineru/auto；MinerU 是同步 POST `/file_parse` | [配置](https://github.com/volcengine/OpenViking/blob/172c1050716f63339f4001292d58c830e6b7a315/docs/en/guides/01-configuration.md)。由异步 Worker 包裹耗时调用 |
| 图片语义描述标为 Experimental；非 Git 导入即使 `wait=false` 仍先完成解析和资源写入 | [资源 API](https://github.com/volcengine/OpenViking/blob/172c1050716f63339f4001292d58c830e6b7a315/docs/en/api/02-resources.md)。不能把上游异步参数当作上传 API 全流程异步 |
| 大图 parser 保存预览/切片而不保存全分辨率原件 | [图片源码](https://github.com/volcengine/OpenViking/blob/172c1050716f63339f4001292d58c830e6b7a315/openviking/parse/parsers/media/image.py)。业务侧必须独立保留原件及坐标变换 |
| 文本向量可索引图片摘要；图片查询要求多模态 embedding，旧图片需要重建对应向量 | [检索 API](https://github.com/volcengine/OpenViking/blob/172c1050716f63339f4001292d58c830e6b7a315/docs/en/api/06-retrieval.md)。分别验收图片文字问答与以图搜图 |
| `search` context 模式暂不支持 `target_uri`；list 模式支持范围；`reason` 会生成记忆 | [检索](https://github.com/volcengine/OpenViking/blob/172c1050716f63339f4001292d58c830e6b7a315/docs/en/api/06-retrieval.md)、[资源](https://github.com/volcengine/OpenViking/blob/172c1050716f63339f4001292d58c830e6b7a315/docs/en/api/02-resources.md)。首轮用限定范围的 list/find，不复制会话到知识库 |
| 内置 HTTP `/mcp` 可提供检索与图片读取，也暴露写入/记忆/删除等通用工具 | [MCP](https://github.com/volcengine/OpenViking/blob/172c1050716f63339f4001292d58c830e6b7a315/docs/en/guides/06-mcp-integration.md)。网关只转发授权的知识业务操作 |

执行前仍需验证：选定部署版本的 MCP schema 能否完整传递范围；图纸/图片解析接口和坐标；视觉及向量模型服务配置、成本与中文工业资料效果；首次部署资源预算。上述检查可先用合成数据在隔离探针中完成，再落正式接口。
