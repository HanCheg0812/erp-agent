# 路线图

当前阶段：业务 Web/API、PostgreSQL、Cube 生命周期及 JCode/DeepSeek Responses 完成本地联调。ERP MCP 已注册两个只读工具并通过真实 ERPNext 列表/明细读取；工具级 EBAC、ERP 连接、平台用户绑定、本地绑定用户凭据 provider、Agent 友好错误契约和真实双用户权限交集已接入，下一阶段按[ERP MCP 下一阶段设计](erp-mcp-next-phase.md)推进正式凭据生命周期、只读生产化、业务推演和受控写入。OpenViking MCP 工具已注册，服务/Embedding 仍未配置。

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

## In Progress

身份与权限设计转为当前优先事项，见 [ERP MCP 与 EBAC](erp-mcp-ebac.md)。工具策略、连接/绑定、本地凭据 provider、ERP 只读结果 envelope 和真实双用户权限交集已落地；当前继续固定生产后台凭据边界、MCP capability 失效语义及撤销/过期轮换。

当前授权的[Web/API/Cube 生命周期联调](web-api-integration.md)和[首轮真实模型联调](deepseek-integration.md)完成。JCode 常驻 Cube v0.7.2 模板的合成案件首轮、续轮及 API/Worker 重启后续轮已完成，宿主机路径保留作对照。

Cube 常驻运行时的后续恢复与容量验收见独立计划：[Cube 常驻 JCode 运行时](cube-resident-runtime.md)。尚无同口径 Cube/宿主机延迟对照。

按用户最新重点推进 [OpenViking RAG 接入](rag-foundation.md)，覆盖扫描/表格与照片/图纸两类图片。原件上传/RustFS 下载已实现；[聊天附件进入 Agent 的契约](chat-attachment-agent-design.md)已设计，消息绑定、真实看图、解析和知识检索仍待实施。

已按 [Skill 与工具组合](skill-tool-design.md)、[可替换解析器](parser-pipeline-design.md)实施第一批，见[联调记录](skill-parser-integration.md)。新增网关/Worker 优先 Rust；实际解析模型对比、队列和知识发布仍待实现。

## Next

1. 按 [ERP MCP 下一阶段设计](erp-mcp-next-phase.md) 将已完成的本地双用户只读验收推进到正式凭据 provider、MCP capability 失效/轮换和目标 ERPNext 版本复核；写工具继续关闭。
2. 按[聊天附件设计](chat-attachment-agent-design.md)绑定消息与已上传原件，验证 DeepSeek Flash/JCode 的真实图片回合；再按 [解析设计](parser-pipeline-design.md)实现真实 ParseBackend、统一产物、文档任务与 Rust Worker/网关，接 OpenViking，验证引用、版本隔离和失败恢复。
3. 按[Cube 常驻 JCode 运行时](cube-resident-runtime.md)继续验证暂停/重建恢复、并发双案件、跨案件越权与 token 撤销，再对照宿主机运行的延迟和资源；同时遵守[存储设计](../../../architecture/session-storage.md)的 PostgreSQL 唯一持久边界。
4. 固定相同解析产物和模型，对比基础向量检索与 OpenViking 的证据召回、引用准确性、拒答、延迟和成本，按错例改善 RAG。
5. 补齐 proposal、执行记录和 ERP 对账契约，实现 Phase B 的 ERP 业务推演工具；通过审批、状态重检和幂等验收后，才选择一个草稿类写工具进行端到端验证。
6. 在业务 Web 接入 ERP 执行记录，完成正常/异常业务闭环和全链路混合负载测试；Cube 基础创建并发已测到 8 路，JCode/Bridge 多会话和稳态容量仍待测。

## Deferred

客户正式 ERP 对接、精细产能优化、研发 CAD/仿真自动化、多节点扩容。

验收详见 [基线](../../baseline/test-and-release-gates.md)。未收集服务器与负载数据前不承诺并发容量。
