# 路线图

当前阶段：业务 Web/API、PostgreSQL、Cube 生命周期及 JCode/DeepSeek Responses 完成本地联调；当前主机的 API、worker、Cube 探针和 Prometheus/Alertmanager/Grafana 已完成 loopback-only 持久部署，证据见[监控部署记录](monitoring-deployment.md)。ERP MCP 已注册两个只读工具并通过真实 ERPNext 列表/明细读取；工具级 EBAC、ERP 连接、平台用户绑定、本地绑定用户凭据 provider、Agent 友好错误契约和真实双用户权限交集已接入，下一阶段按[ERP MCP 下一阶段设计](erp-mcp-next-phase.md)推进正式凭据生命周期、只读生产化、业务推演和受控写入。OpenViking MCP 工具已注册，服务/Embedding 仍未配置。

## Done

- Cube 基础模板并发探针：现有轻量模板在单健康节点完成 1/2/4/8 路各三轮创建、健康检查及释放；45 个实例全部成功且清理归零。仅为生命周期小规模验证，不代表 JCode 业务容量；详见[测量记录](cube-concurrency.md)。

- 建立独立项目骨架、协作说明和计划入口。
- 收录最终 Cube + JCode 架构图、架构说明及业务 Prompt 草案。
- 记录已确认决策和下一步验证顺序。
- 按 auth/cases/approvals/audit 组织 API，采用 PostgreSQL，迁移、初始化及本地启动可运行。
- 方案版本、审批摘要绑定、并发幂等及事务审计；真实数据库集成验证通过。
- 固定 JCode submodule 与上游检查命令；保留独立更新能力。
- Cube 官方两种部署方式及常驻预热研究，记录待实测的推荐方案。
- 灰白 Chat 主界面、真实登录/案件/消息持久化/审批，桌面与手机验证通过。
- Cube 存储、节点、模板和代理故障修复；案件绑定、创建幂等、释放核验、过期清理和 API 进程重启恢复通过。
- 按授权清理 Mindloop 专属资源，保留 Cube 与其他项目；详见[记录](mindloop-cleanup.md)。
- 原生 DeepSeek Responses 经 Rust JCode Loop 完成案件读取、todo 和进程重启恢复；思考片段与工具调用按顺序保存，Web 支持折叠查看和流式打字。
- 业务 Skill 目录、发现/加载、版本归档接通现有 Loop，真实模型与 API 重启验证通过；修复原生 reasoning 历史和并行工具排序。
- Rust ParseBackend、单父配置继承、阶段依赖、稳定快照与实验标识库通过 5 项 fixture 测试；实际 Worker/解析服务尚未接入。

- Worker 并发调度与 PostgreSQL 队列已完成受控验证；该结果不代表 Cube 内 Agent 容量。
- 当前 Cube 主机持久部署：8 个用户级 worker、30 秒健康快照、Prometheus/Alertmanager/Grafana、Cubelet 原生资源采集与 11/11 `up` 目标已实测；14 条规则通过校验，Alertmanager canary 已投递到脱敏本地 sink。仅覆盖单主机 loopback 运维，不代表公网生产、高可用或外部通知。

## In Progress

身份与权限设计转为当前优先事项，见 [ERP MCP 与 EBAC](erp-mcp-ebac.md)。工具策略、连接/绑定、本地凭据 provider、ERP 只读结果 envelope 和真实双用户权限交集已落地；当前继续固定生产后台凭据边界、MCP capability 失效语义及撤销/过期轮换。监控基线已部署，仍需外部通知、远端备份和故障演练。

当前授权的[Web/API/Cube 生命周期联调](web-api-integration.md)和[首轮真实模型联调](deepseek-integration.md)完成。JCode 常驻 Cube v0.7.2 模板的合成案件首轮、续轮及 API/Worker 重启后续轮已完成，宿主机路径保留作对照。

Cube 常驻运行时已完成同实例暂停恢复、销毁重建和 50 用户合成并发探针；后续同口径性能归因、长期混合负载和恢复边界见独立计划：[Cube 常驻 JCode 运行时](cube-resident-runtime.md)。尚无同口径 Cube/宿主机延迟对照。

[Cube 模板接入](runtime-management.md)按用户澄清重新收敛：让 JCode 复用 Cube 的不同模板环境。受控代码工具已通过多轮/重建恢复，受控浏览器工具已通过一个真实网页读取回合及 PostgreSQL/回收核对。原生 Cube Web 可选择 READY 模板创建实例；Rust API 现已补齐受控的 Forge/JCode 会话切换流程：创建新模板实例、等待 guest ready、迁移同一 PostgreSQL 会话并释放旧实例。运行中实例仍不支持原地换模板；业务 Web 后续只需调用窄接口显示切换状态，不复制 Cube 模板管理。继续补齐浏览器故障/并发及业务工具摘要验收，停止扩展尚未提交的 Forge Runtime 管理页实验。

按用户最新重点推进 [OpenViking RAG 接入](rag-foundation.md)，覆盖扫描/表格与照片/图纸两类图片。聊天附件的 RustFS 原件、消息绑定、JCode 图片输入和一张合成图片的真实问答已验证，见[附件记录](chat-attachment-agent-design.md)；持久解析、入库和知识检索仍待实施。

已按 [Skill 与工具组合](skill-tool-design.md)、[可替换解析器](parser-pipeline-design.md)实施第一批，见[联调记录](skill-parser-integration.md)。新增网关/Worker 优先 Rust；实际解析模型对比、队列和知识发布仍待实现。

## Next

1. 按 [ERP MCP 下一阶段设计](erp-mcp-next-phase.md) 将已完成的本地双用户只读验收推进到正式凭据 provider、MCP capability 失效/轮换和目标 ERPNext 版本复核；写工具继续关闭。
2. 在已验证的聊天附件和合成图片问答基础上，按[解析设计](parser-pipeline-design.md)实现真实 ParseBackend、统一产物、持久文档任务与 Rust Worker/网关，接 OpenViking，验证两类图片的引用、版本隔离和失败恢复。
3. 按[Cube 常驻 JCode 运行时](cube-resident-runtime.md)补齐 guest 工作集和同口径宿主机/Cube 对照，将回复正确率与延迟一起验收；继续验证写操作断线、跨节点迁移和长期混合负载，同时遵守[存储设计](../../../architecture/session-storage.md)的 PostgreSQL 唯一持久边界。
4. 固定相同解析产物和模型，对比基础向量检索与 OpenViking 的证据召回、引用准确性、拒答、延迟和成本，按错例改善 RAG。
5. 补齐 proposal、执行记录和 ERP 对账契约，实现 Phase B 的 ERP 业务推演工具；通过审批、状态重检和幂等验收后，才选择一个草稿类写工具进行端到端验证。
6. 在业务 Web 接入 ERP 执行记录，完成正常/异常业务闭环和全链路混合负载测试；Cube/JCode 已完成 50 路合成短时探针，稳态容量仍待测；持续运行当前监控并补齐外部通知、远端备份与多节点方案。

## Deferred

客户正式 ERP 对接、精细产能优化、研发 CAD/仿真自动化、多节点扩容。

验收详见 [基线](../../baseline/test-and-release-gates.md)。未收集服务器与负载数据前不承诺并发容量。
