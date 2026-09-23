# 路线图

当前阶段：业务 Web/API、PostgreSQL、Cube 生命周期及 JCode/DeepSeek Responses 完成本地联调，ERP/知识工具待接入。

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

## In Progress

当前授权的[Web/API/Cube 生命周期联调](web-api-integration.md)和[首轮真实模型联调](deepseek-integration.md)完成。JCode 当前在主机独立进程运行，尚未加入 Cube 模板。

按用户最新重点推进 [OpenViking RAG 接入](rag-foundation.md)，覆盖扫描/表格与照片/图纸两类图片。原件上传/RustFS 下载已实现；[聊天附件进入 Agent 的契约](chat-attachment-agent-design.md)已设计，消息绑定、真实看图、解析和知识检索仍待实施。

已按 [Skill 与工具组合](skill-tool-design.md)、[可替换解析器](parser-pipeline-design.md)实施第一批，见[联调记录](skill-parser-integration.md)。新增网关/Worker 优先 Rust；实际解析模型对比、队列和知识发布仍待实现。

## Next

1. 按[聊天附件设计](chat-attachment-agent-design.md)绑定消息与已上传原件，验证 DeepSeek Flash/JCode 的真实图片回合；再按 [解析设计](parser-pipeline-design.md)实现真实 ParseBackend、统一产物、文档任务与 Rust Worker/网关，接 OpenViking，验证引用、版本隔离和失败恢复。
2. 按[存储设计](../../../architecture/session-storage.md)补齐长上下文压缩与 Cube 重建恢复；建立 Cube 常驻 JCode/Bridge 模板，验证双会话隔离及暂停恢复，对照外部运行的延迟和资源。
3. 固定相同解析产物和模型，对比基础向量检索与 OpenViking 的证据召回、引用准确性、拒答、延迟和成本，按错例改善 RAG。
4. 补齐执行记录/ERP 对账契约，实现 ERPNext 合成数据和 ERP 业务工具，验证批准后重新检查 ERP 状态。
5. 在业务 Web 接入 ERP 执行记录，完成正常/异常业务闭环和全链路混合负载测试；Cube 基础创建并发已测到 8 路，JCode/Bridge 多会话和稳态容量仍待测。

## Deferred

客户正式 ERP 对接、精细产能优化、研发 CAD/仿真自动化、多节点扩容。

验收详见 [基线](../../baseline/test-and-release-gates.md)。未收集服务器与负载数据前不承诺并发容量。
