# RAG 开源实践调研

日期：2026-09-22。模式：shape-plan。范围：MinerU 官方 Skill、文档阅读 Agent、OpenViking 集成和多模态 RAG。本文依据公开文档、源码与测试定义；没有安装或运行这些项目，也不将作者演示视为本项目效果验收。

结论：已有实践可以复用。最值得借鉴的是“检索候选 -> 按需深读 -> 取表格/图片证据 -> 回答”的工具循环，以及解析结果的可定位引用。继续使用现有 JCode、PostgreSQL 和 OpenViking 方向，先做受控工具与解析探针；不因此增加第二套正式知识索引或更换 Harness。接入方案见 [RAG 基础](rag-foundation.md)。

## 1. MinerU 官方 Skill：按位置继续读取

来源：[官方 SKILL.md](https://github.com/opendatalab/MinerU/blob/94828a88d80ab8a72c21956de0604bf14f4cc9fc/skills/mineru/SKILL.md)。研究快照 `94828a88d80ab8a72c21956de0604bf14f4cc9fc`，并非已选部署版本。

- 当前文档针对 MinerU 4.x，首次 `parse`，已有 locator 后使用 `read`，长文件沿 `next_request` 继续读取。
- 定位包含文档、解析档位、页、块、字符偏移，例如 `doc:ab12cd3/tier:standard/page:4/block:7`；页和块从 1 开始，字符偏移从 0 开始。
- 支持有界输出、邻近上下文、按页/块读取图片。PDF 块转图片依赖有效 bbox；不能假定任意格式的任意块都能返回图像。
- Skill 的价值包括工具选择、续读、质量档位和来源保留，不只是把文件转成 Markdown。文档扫描件 OCR 支持不等于产品照片理解或工程尺寸识别已经可靠。

本项目借鉴：保存原生 locator 与解析结果映射，搜索后继续读取具体证据。业务版本、文件哈希和解析配置共同确定来源；不要把 parser 缓存 ID 或块号当作跨重解析永不变化的业务主键。官方 CLI Skill 不直接作为产品 Agent 的 Bash 权限：解析由 Worker 执行，在线通过受控 MCP 暴露业务读取能力。

兼容性待验证：此前核查的 OpenViking 内置 MinerU PDF 适配使用 `/file_parse`。不能把 MinerU 4.x Skill 的 CLI/Doclib 契约与旧接口直接视为兼容，需先锁定服务版本并验证产物和恢复行为。

## 2. MinerU Document Explorer：阅读、综合与知识整理

来源：[官方项目](https://github.com/opendatalab/MinerU-Document-Explorer/tree/a7e9c6cc25b7edbf4ebd35aea8e270523a8a3e40)、[Skill](https://github.com/opendatalab/MinerU-Document-Explorer/blob/a7e9c6cc25b7edbf4ebd35aea8e270523a8a3e40/skills/mineru-document-explorer/SKILL.md)、[演示](https://github.com/opendatalab/MinerU-Document-Explorer/blob/a7e9c6cc25b7edbf4ebd35aea8e270523a8a3e40/demo/README.md)。

该项目使用 QMD，提供 15 个 MCP 工具。文档描述的检索组合是 BM25、向量、查询扩展与重排；阅读工具包含 `doc_toc`、`doc_read`、`doc_grep`、`doc_query`、`doc_elements`。先定位文档，再读章节或地址，表格/图片可以单独提取。另有 Wiki 编写、来源字段和失效链接检查。

具体演示是把 10 篇 arXiv RAG 论文整理成论文摘要、概念页、关联 Wiki 和研究综述。项目还给出设计文档问答、教材整理的使用脚本；这些是示例用途，不是有独立验收数据的客户部署。

本项目借鉴：

1. 料号、图号、修订等精确查询和语义问题分开评测；摘要用于导航，关键结论继续深读原文。
2. 保留目录、父段落、完整表头、图注与邻近上下文，减少孤立切块造成的误读。
3. 回答中绑定读过的来源与位置；生成的综合笔记与原始证据区分，不让二次总结替代原件。

Document Explorer 本身不是 OpenViking 适配器。可借鉴其 Skill 与工具契约；暂不在正式路径中同时部署 QMD 和 OpenViking。Wiki 编写不是当前业务读取工具的必要能力。

## 3. AgentRag-OpenViking：DeepSeek 知识问答 Loop

来源：[社区项目](https://github.com/jidechao/AgentRag-OpenViking/tree/8a9b035a29c979a2f698c94e6afa5d0ae7d5e6bf)、[运行时源码](https://github.com/jidechao/AgentRag-OpenViking/blob/8a9b035a29c979a2f698c94e6afa5d0ae7d5e6bf/agentic_rag/agent.py)、[引用提取](https://github.com/jidechao/AgentRag-OpenViking/blob/8a9b035a29c979a2f698c94e6afa5d0ae7d5e6bf/agentic_rag/events.py)、[事件测试](https://github.com/jidechao/AgentRag-OpenViking/blob/8a9b035a29c979a2f698c94e6afa5d0ae7d5e6bf/tests/test_agent_event_stream.py)。

组合为 Claude Agent SDK + DeepSeek + OpenViking 原生 HTTP `/mcp`，提供终端问答、文件任务和 REST/SSE。示例资料包括产品参数、运维手册和研究结论；README 给出单事实、多文档汇总和无依据问题三类检查。

源码可确认：

- 运行时限制为 OpenViking 只读工具，并通过模型循环决定 `find/search/read`；没有固定调用两次的业务流程。
- 提示词要求先检索、必要时读详情、引用 `viking://`、缺乏依据时拒答。
- 事件区分思考、正文、工具调用/结果、引用；测试覆盖流式切分后的 URI 提取，但所查测试使用假的运行时。
- `_prompt()` 把 `target_uri` 写入提示词，所查引用函数从字符串提取 URI。这两点本身不能证明案件权限强制生效或引用实际支持回答。

本项目借鉴其流式事件与引用展示；继续用 JCode，不引入 Claude 运行时。范围由后端身份和案件数据构造；引用必须映射到本次已授权读取的原件与版本。无依据拒答需要专门反例评测，不能凭提示词或 URI 格式认定已通过。

## 4. OpenViking 官方集成：可以接现有聊天和外部解析器

来源均为 OpenViking 研究快照 `172c1050716f63339f4001292d58c830e6b7a315`：

| 示例 | 实际说明 | 对本项目的启发 |
| --- | --- | --- |
| [OpenWebUI](https://github.com/volcengine/OpenViking/tree/172c1050716f63339f4001292d58c830e6b7a315/examples/openwebui-plugin) | FastAPI/OpenAPI 工具服务把搜索、读取等操作接到已有聊天产品；该示例不是 MCP 传输 | 现有 Chat 通过工具接知识库即可；不用重做一个聊天系统 |
| [LangChain/LangGraph](https://github.com/volcengine/OpenViking/tree/172c1050716f63339f4001292d58c830e6b7a315/examples/langchain) | `OpenVikingRetriever`、工具及多种记忆适配，SDK 连接独立服务 | 检索适配可独立使用；本项目不引入其会话/长期记忆存储 |
| [LlamaParse bridge](https://github.com/volcengine/OpenViking/tree/172c1050716f63339f4001292d58c830e6b7a315/examples/llamaparse-understanding-bridge) | 外部解析任务映射到 Understanding API，以 Markdown 和图片 ZIP 导入 | 外部解析产物接入已有参考；仍须自行保存原件、块坐标及业务版本 |

LlamaParse 示例允许部分页面失败时保留可用内容并加提示；本项目发布策略必须另行校验所需页面完整性。上述示例的认证、单租户环境和模拟测试不能替代业务权限及恢复验收。

## 5. RAG-Anything：图表和文字一起理解

来源：[项目](https://github.com/HKUDS/RAG-Anything/tree/1f73f0154c48b3d616674608af98748585b856f3)、[上下文感知处理](https://github.com/HKUDS/RAG-Anything/blob/1f73f0154c48b3d616674608af98748585b856f3/docs/context_aware_processing.md)。

它基于 LightRAG，接入 MinerU 解析结果，对图片、表格、公式分别处理，再建立多模态关联与检索。文档明确提供页面/块上下文窗口、标题和图注选项，且允许接入已经解析好的 content list。它不是 OpenViking 集成案例。

本项目借鉴多模态证据组织：图纸区域带标题栏、单位与对应标注，表格带表头，照片保留原图和独立视觉解释。先验证这种上下文补充的收益，再判断是否需要额外图检索；项目的通用效果描述不能代表工业图纸数值准确率。

## 对现有方案的具体调整

- 检索工具返回候选与可读定位；读取工具支持目录/正文块/表格/图块及有限邻近上下文，按当前任务选择读取，不预设固定工具次数。
- 来源映射记录业务文档版本、原件哈希、解析器/配置版本、原生 locator、块坐标和 OpenViking URI。Web 使用业务来源 ID 打开证据，模型不能自行指定任意文件路径。
- 首轮探针先验证 MinerU API/产物兼容，再比较“直接读召回片段”与“检索后深读”，分别统计表格、图纸、版本冲突与无答案样本。
- 有效版本筛选、客户隔离及引用核验属于后端；仍用同一批原件、解析产物和模型比较基础检索与 OpenViking。尚无本项目数据证明哪种方案更好。

下一步：在既有两类图片评测样本上跑解析与来源定位探针，再落正式上传/异步入库接口。当前未选择额外模型服务，也未安装第三方 Skill 或启动以上项目。
