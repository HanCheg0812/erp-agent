# 聊天附件进入 Agent 与知识库

日期：2026-09-23。本文记录已落地的聊天附件即时读取路径及尚未落地的知识路径。图片问答已用真实 DeepSeek Flash 验证；MinerU 解析和 OpenViking 索引仍待接入。

相关：[RAG 基础](rag-foundation.md)、[解析流水线](parser-pipeline-design.md)、[会话存储](../../../architecture/session-storage.md)、[路线图](roadmap.md)。目标是让用户在一条消息里上传图片或文件、提问，并能区分 Agent 已看到图片、正在解析文件、已检索到证据这三件事。

## 已核对的边界

- 固定 JCode `2a4edaa02057ac994a601311c4f03ed450e1b3c9` 的 `ContentBlock::Image`、`Message::user_with_images` 和 `Agent::run_once_streaming_mpsc(text, images, ...)` 已有图片输入；OpenAI Responses provider 把用户图片序列化为 `input_image` 的 base64 data URL，并有不支持图片时的出站过滤。JCode 负责模型回合、工具循环和会话，不负责 Forge 的文件授权、RustFS、解析任务、业务版本或引用定位。
- [DeepSeek 图像理解文档](https://api-docs.deepseek.com/zh-cn/guides/vision)在 2026-09-23 核对：`deepseek-flash` 支持 Responses API `input_image`；JPEG、PNG、GIF、WebP 可用，图片计入请求体和 token 预算。当前仅对 Flash 开放即时图片输入；`deepseek-v4-pro` 的视觉能力不能由 Flash 文档推定。
- Forge `POST /cases/:id/documents` 将 PDF/DOCX/XLSX/MD/TXT/PNG/JPEG/WebP 原件存入 RustFS，并支持按案件授权下载；`ready` 仅表示原件已保存。聊天消息可绑定最多四个有序附件，同一事务校验案件/所有者/状态/总大小并持久化引用。Worker 将原图和已提取的有界文件内容送入当前 JCode 回合；Agent 可通过 `attachment_read(document_id, offset/page/image_offset)` 继续读取同案件原件。此处的“已读取”不代表持久索引已完成。
- OpenViking 搜索/深读工具已有受控接口的准备工作，真实索引、解析 Worker、embedding 服务尚未连通。不能把文档 `ready` 与知识 `ready` 合并成一个状态。

## 一条消息，两条处理路径

```text
选择/粘贴/拖入图片或文件
  -> Web 上传原件 -> RustFS + PostgreSQL document_id
  -> 发送 text + attachment_ids，API 在同一事务中绑定消息/附件并排入 Agent 回合
      |-> 即时读取路径：Rust 提取 DOCX 正文/图片、XLSX 表格坐标、PDF 页文本/页图及 MD/TXT；不完整时 Agent 按游标续读
      |-> 即时图片路径：符合模型能力和大小预算时，从 RustFS 取图交 JCode images
      |-> 异步知识路径：Rust Worker 解析 -> 带位置的证据 -> OpenViking 发布
  -> 后续 Agent 通过受控 MCP search/read 深读已发布证据，并返回可点击来源
```

图片即时路径用于当前回合看图、识别截图和初步解释；它不把视觉回答自动变成已核实业务证据。知识路径用于 PDF、长文、表格、图纸和可复用图片，依赖可追溯的解析/版本/检索。两条路径可同时处理同一图片，但 UI 和回答必须说明所用来源及处理状态。文档解析失败不应抹掉已保存原件；即时看图成功也不意味着文档已入库。

## 消息与存储契约

1. `conversationInput` 增加可选、有序、去重且数量受限的 `attachment_ids: UUID[]`；允许 `text` 非空或附件非空。首批保持现有 5 MiB 上传上限，先实测图片请求体/像素/token，再调整限制；不要直接照搬提供商上限。上传不自动发送消息。发送前 Web 展示缩略图/文件名、移除、上传中/失败状态，未完成上传不能发送对应附件。
2. 新增 `conversation_message_attachments(tenant_id, case_id, message_id, document_id, ordinal, source_sha256, ...)` 或等价关系。API 在保存消息/排队的同一事务里校验每个文档的租户、所有者、案件、原件状态及哈希；同一个 `client_message_id` 重试要核对文字、设置和有序附件快照。消息历史与运行记录返回附件元数据，绝不返回原件字节或 RustFS key。
3. 原件继续不可变保存在 RustFS；PostgreSQL 保存引用、版本、状态及审计。业务消息表不再复制图片 base64；但固定 JCode 的图片块会随会话持久化到 PostgreSQL，不能声称只存引用。运行前由可信 Worker 按消息绑定重新授权、校验哈希并读取原件；不得接受模型或客户端给出的任意对象 key、外部 URL、租户 ID 或本地路径。
4. 恢复语义要明确：已绑定的消息在重试/进程重启后仍是同一组附件。Bridge 对新回合可传 `images` 给 JCode；若回合已进入 JCode 会话，恢复应延续该会话，不能把附件作为第二条用户消息重复送入。测量 JCode 会话中的图片 base64 体积、后续请求重送及压缩行为；长期依据应保留文档/来源 ID 与有界摘要，原件仍由 RustFS/证据工具读取。不能仅靠 JCode 的紧急图片截断维持正常上下文预算。

## 即时图片路径

- 首轮仅对已验证支持视觉的模型开放。启动时/切换模型时显式配置并探测 `supports_image_input`，先用合成 PNG 做真实 Responses 回合测试；JCode provider 的宽松布尔值不能替代 DeepSeek 各模型的实际能力测试。文本模型遇到图片时不能悄悄降级为“已看图”：显示“图片已上传，当前模型未分析图片”，可等待异步解析或让用户切到已验证模型。
- API/Worker 对实际文件头、MIME、像素、张数、总请求体和预计上下文预算做校验，必要时生成有哈希和原图坐标映射的受控衍生图。传 JCode 的是合规图片数据，不是 RustFS URL；缩放图不能冒充原件精度。PDF/TXT 不伪装成 `Image`，走解析及按需读取。
- Bridge 的 stdin 上限已调至 12 MiB；Worker 仅从已绑定、重新校验大小和 SHA-256 的 RustFS 对象构造图片输入。Bridge 再验证图片 MIME、张数和单张编码大小。当前没有像素重采样或多图大负载实测；同机/沙箱内二进制通道仍是后续优化，不作为已落地能力。
- 模型看到图片后输出“从图上看到”的初步描述；数量、日期、尺寸及修订结论仍需解析块或原图区域复核，尤其是表格/图纸。不得以普通对话描述自动填入审批方案的正式证据。

## 异步解析与检索路径

- 原件确认落盘后写持久解析任务。Worker 依 [解析流水线](parser-pipeline-design.md) 选择 MinerU/OCR/视觉后端，保留页、表格行列、图块、父子关系与原图定位；失败、重试、配置版本和租约可恢复。发布到 OpenViking 前先校验完整性和业务适用版本。
- 搜索工具只召回当前授权案件下已发布且适用的版本；深读工具按 `source_id` 返回有界正文、表格或受控图块及稳定 locator，并再次复核权限。模型不能遍历 RustFS，也不能直接把“同案件所有上传件”塞进上下文。引用点击仍由 API 按来源坐标打开原件。
- 对刚上传但尚未索引的附件，Agent 可用受限 `attachment_read` 读当前授权案件的原件，并明确这不是已发布知识证据；需要跨会话复用的结论仍须等待索引 `ready`。不能拿历史已发布版本替代当前附件而不说明版本差异。
- 状态分层：原件 `uploading/stored/failed`；解析/索引 `queued/parsing/validating/indexing/ready/needs_review/failed`；当前回合另记 `vision_sent/vision_unavailable/vision_failed`。旧 API `documents.state=ready` 暂按 `stored` 解释，迁移时避免让已有文件凭旧状态自动变成“可检索”。

## 交互与验收

`+` 菜单保持“上传文件/上传图片”。选择后附件留在输入框上方的紧凑列表，可预览/移除；发送成功后归属该条消息，历史刷新仍可见。拖放、粘贴及跨设备 UI 验收尚未完成。附件状态与 Agent 工具步骤分开显示；只有真实 `search/read` 返回的来源才显示为引用。“上传完成”不写成“Agent 已阅读”。

## 2026-09-23 验证

- `npm run check` 覆盖事务绑定、附件顺序与幂等、无文字发图、非 Flash 图片拒绝、历史引用及同用户跨案件/同租户跨所有者拒绝；Runtime Bridge 的 Rust 测试通过。
- `scripts/verify-chat-image-live.mjs` 使用独立 Rust API 进程、PostgreSQL、RustFS、JCode 和真实 DeepSeek Flash Responses 回合；6,712 字节的合成 PNG 中的四位数字为 `7392`，模型回复 `7392`，历史消息保留了同一附件 ID。该测试不证明表格/图纸识别准确率、多图性能或压缩后的图片恢复行为。
- `npm run parser:test` 覆盖中文长文本续读、Word 表格/图片游标、Excel 单元格定位和两页 PDF 续读；`npm run check` 覆盖 Rust API、Web 构建及 PostgreSQL 系统回归。新增系统用例验证 20,000 字附件游标、审计、跨案件拒绝及浏览器 token 不可冒用 Agent token。以上测试使用合成文件与存储 fixture，不等于真实 DeepSeek 对全部文件格式的端到端验收。
- `scripts/verify-chat-document-live.mjs` 通过当前 3100 API 上传 28,557 字合成 Markdown；真实 DeepSeek Flash 回合调用一次 `attachment_read`，取得首段之外的随机代号并正确回答。故障排查中发现旧 3100 进程曾拒绝新版 `attachment_ids` 字段；模型工具调用中的可选游标为 JSON `null` 时，Bridge 又将其传给要求整数的 Rust API。重启 3100 服务、在 Bridge 清理空游标并补回归测试后，该端到端用例通过。4173 的 `/v1` 代理已核实指向当前 API。本次没有覆盖所有文件格式的真实模型问答。
- 原件上传完成、即时读取与知识索引是三种不同状态。OpenViking 服务、Embedding、MinerU 解析 Worker 尚未在此链路验收。

按以下顺序实施并留存请求/响应、截图及数据库/存储核验：

1. 消息附件契约、事务绑定、幂等和 Web composer；验证图片/文件独立发送、附文发送、删除草稿、刷新、跨案件和跨租户拒绝。
2. 小图即时路径：DeepSeek Flash 合成图片问答、模型不支持/图片超限提示、Bridge/JCode/主机重启后无重复消息或丢图；记录 TTFT、图片 token、费用、JCode 会话字节数及下一回合请求体大小。未通过真实探针前不在 UI 宣称“能看图”。
3. PDF/扫描表格/照片/图纸各至少一个解析样本，Worker 中断/重试和版本冲突反例通过；接 OpenViking 的受控 search/read 与原文定位，确认没有跨租户或失效版本泄漏。
4. 同一问题对比“直接看图”“解析后深读”“两者结合”：逐题记录关键字段、证据引用、拒答、延迟、成本。ERP 实时事实另由 ERP 工具验证，不由 RAG 代替。

未决配置：专用视觉模型探针以 Flash 起步；embedding 服务、MinerU 版本和真实 Worker 部署仍需配置。部署与性能验收不得从接口代码存在推断。
