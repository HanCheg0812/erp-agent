# Cube 基础创建并发探针

测试日期：2026-09-23。结果：现有 Cube 基础模板在 8 路并发创建下全部成功；业务 Agent 并发仍未知。

## 环境

- Cube API/runtime v0.4.0，健康节点 `192.168.100.122`。
- 节点报告配额：48,000 mCPU、79,652 MiB 内存、155 个 MVM slot；节点查询时无活动实例。
- 模板 `tpl-erp-agent-base-20260921`，READY replica 规格 `cpu=2000m,mem=2000Mi`。模板仅运行 lightweight code interpreter，不含 JCode 或 Runtime Bridge。
- 创建请求禁用互联网访问，使用每次运行唯一的 `probe_run` 元数据和 900 秒 TTL。探针只对自身唯一标记的实例执行 DELETE。

## 方法与结果

`tests/performance/cube-concurrency.mjs` 分别以 1、2、4、8 个同时请求为一轮，每级重复三轮。每个创建请求记录 Cube API 返回时间，并持续经代理请求 guest `/health`，直到返回 `OK` 或 15 秒超时；一轮完成后删除所有本轮实例并通过列表确认归零。每轮开始前要求 Cube 实例总数为 0。

| 并发创建 | 轮次 | 成功健康 | 创建响应 P50 / P95 | 代理健康就绪 P50 / P95 |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 3 | 3/3 | 62 / 63 ms | 66 / 84 ms |
| 2 | 3 | 6/6 | 75 / 77 ms | 79 / 88 ms |
| 4 | 3 | 12/12 | 61 / 77 ms | 65 / 83 ms |
| 8 | 3 | 24/24 | 78 / 83 ms | 82 / 92 ms |

总计 45/45 个实例成功通过健康检查，12/12 轮释放后均为零残留。每轮墙钟约 0.89-1.01 秒，其中约 0.82-0.92 秒为并发删除及 provider absence 核验。宿主机 `MemAvailable` 从 40,037 MiB 变为 39,807 MiB，探针测量时最低 39,722 MiB；这不是 guest RSS 或严格归因的资源增量。SwapFree 没有下降。

完整逐请求结果、主机内存采样及运行 UUID 保存在 [机器可读证据](cube-concurrency-evidence-erp-agent-cube-bench-e7618602-ab7a-4161-ae7d-d2457bc4fab1.json)。脚本检查点包括测试前实例清空、健康节点和模板副本匹配、按唯一标签过滤清理目标及结束后再次核验。

## 结论边界

当前只能说这个健康节点上的 lightweight 模板已实测支持 8 路同时创建并达到 guest health，且这组短测没有错误。数据不覆盖稳态活跃实例、超过 8 路时的饱和点、多租户混合流量、重启/恢复、JCode/Bridge 初始化、DeepSeek 推理或 ERP/RAG 工具工作负载。创建响应及健康探针延迟也不是模型 TTFT。节点报告的 155 slots 和 CPU/内存是调度配额，不是已经验证的并发容量。

因此不把 8 路外推成 Agent 并发承诺。下一轮要先在模板中加入 JCode/Bridge，再固定模型配置，用双案件隔离和逐级并发回合同时测成功率、排队、TTFT/P95、活跃及暂停资源与恢复正确性。
