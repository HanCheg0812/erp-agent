# 存储与状态

PostgreSQL 已保存租户、用户、令牌哈希、案件、方案、审批、审计、个人记忆、Agent 任务和文档元数据；开发 Compose 固定 postgres:17.6-bookworm。RustFS 私有桶保存用户/案件范围的原件，内容哈希、越权拒绝与服务重启持久化已验证。真实 ERP 执行台账、解析持久队列和知识发布尚未实现。数据库及对象存储配置由本地 .env 提供，不提交凭据。

JCode 会话、消息、todo、工具结果与压缩检查点已通过远程存储适配写入 PostgreSQL；真实模型循环、长上下文压缩及 API 进程重建后恢复通过，运行时未发现 session/todo JSON/JSONL 文件。沙箱只保存运行中内存与可重建临时状态；快照不是持久会话的权威来源。Harness 目前在宿主机运行，Cube 重建恢复仍待单独验收。详见[存储设计](../../architecture/session-storage.md)。todo 不是业务完成状态。沙箱回滚不会撤销外部 ERP 写入。
