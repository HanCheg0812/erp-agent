# 待确认信息

- 目标服务器：CPU、内存、GPU/显存、操作系统、KVM/PVM、磁盘和文件系统。
- DeepSeek 原生 Responses 的长期错误率、费用和容量；专用配置及 case_read/todo 调用已经实测通过。
- 业务用户并发、在线响应目标、文档吞吐目标与样本量。
- Cube/SDK/OpenViking/MinerU/ERPNext 的实施版本和兼容组合；JCode 已固定 commit 并经 Rust Bridge 运行，SDK/socket 路径未接入。
- RAG 所需专用视觉与向量模型服务、图片/图纸解析版本和坐标能力；首批资料已确认两类都有，具体模型及资源预算待验证，见 [RAG 方案](rag-foundation.md)。
- 记忆与知识的云端架构基线见[记忆与 OpenViking 架构](memory-openviking-architecture.md)：首期 PostgreSQL/pgvector 负责个人记忆权威状态与召回，OpenViking 负责企业 Resource/Wiki/RAG；仍需实测 embedding 维度、PG 带过滤召回、客户共享记忆授权和个人记忆投影收益。
- JCode 在 Cube 内/外的实际延迟、实例资源和恢复正确性；目前优先验证常驻沙箱内方案。
- 沙箱持久卷或检查点方案，以及运行时销毁重建后的恢复契约。
- 正式客户业务的数量口径、版本优先级、料号映射和人工审批规则。

上述信息不阻止先做接口研究与合成样本验证，但会影响正式部署、容量和业务验收。
