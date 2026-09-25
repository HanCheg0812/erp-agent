# Cube 常驻 JCode 运行时

状态：本机 Cube 已升级至 v0.7.2；Phase 0/1/2 的常驻 JCode 路径已在真实 Cube guest 完成首轮、同案件续轮和 API/Worker 重启后续轮验证。两个合成案件使用不同实例与 PostgreSQL 会话；销毁重建、同一实例三次暂停恢复、同案件串行/幂等和 50 用户真实并发均已通过。8 个独立 worker 每个 8 槽，总计 64 个调度槽位；此前 50 用户测试只完成 32 个的根因是启动单元使用了错误的 `AGENT_WORKER_MAX_CONCURRENT_RUNS` 名称，Rust 实际读取的 `AGENT_MAX_CONCURRENT_RUNS` 已修正。宿主机 Harness 保留作回归基线。本文只描述 Agent 运行路径，不改变 ERP/OpenViking、RustFS 或文档解析的服务边界。

## 目标与非目标

目标：

- 一个活跃案件会话绑定一个 Cube 实例；同一会话的多轮输入复用同一个 Bridge/JCode 进程。
- Worker 继续负责 PostgreSQL 队列、案件 advisory lock、并发槽位、停止和恢复；它不再直接启动正式路径的宿主机 Bridge。
- Cube 只承载运行时隔离、临时工作目录和进程生命周期；PostgreSQL 仍是消息、工具、todo、压缩检查点和运行状态的唯一持久来源。
- 模型服务、ERP MCP、OpenViking、RustFS 和解析 Worker 保持在沙箱外，由受限的内部通道访问。
- 记录可分解的队列、Cube、Bridge、模型和工具时间，分别比较宿主机基线、Cube 暖实例、干净创建和暂停恢复。

非目标：

- 不把 Cube 控制面、模型密钥、ERP 高权限凭据或数据库通用账号暴露给浏览器或业务模型。
- 不为每条消息或每次 MCP 调用创建新的沙箱。
- 不在本阶段实现 ERP 写入、OpenViking 索引或 MinerU 真实解析。

## 目标运行路径

```text
Web -> Rust API -> PostgreSQL agent_runs
                    -> Worker
                    -> Cube create/resume/bind
                    -> authenticated guest Bridge
                    -> resident JCode
                    -> scoped MCP gateway / shared model service
                    -> PostgreSQL checkpoint and events
```

Worker 是调度层，Cube 是隔离层。Worker 可以有多个进程和槽位，但同一案件仍由数据库租约和 advisory lock 保证单活；不同案件可以并行使用不同 Cube 实例。

## 分阶段实施

### Phase 0：运行契约和模板探针

- 固定 JCode、Runtime Bridge、MCP gateway 的版本和启动参数。
- 生成专用 Cube 模板，启动一个不带用户数据和长期凭据的 Bridge。
- 增加 `/health/ready`，至少确认 Bridge 进程和运行配置可以接收请求；首次绑定案件会话后再用 `agent_initialized` 表示 JCode、PostgreSQL session 和受限 MCP 注册已经完成，不能只检查 Cube guest 端口。
- 用合成请求验证 guest 内 Bridge 的回环、鉴权、单请求串行和优雅停止。

已完成：`services/runtime-bridge` 在 `AGENT_RUNTIME_MODE=resident` 下提供 `/health/live`、`/health/ready` 和带单会话互斥的 `/v1/turn`；首次 turn 在 guest 进程内建立并持有 JCode `Agent`、PostgreSQL session、技能目录和 ERP/OpenViking MCP 注册表，后续 turn 复用这些对象。`/health/ready` 报告 `resident_agent: true`，并用 `agent_initialized` 区分尚未绑定会话和已经完成真实初始化的实例；宿主机 stdin/stdout 入口仍保留作为回滚路径。

剩余完成条件：验证实例之间不能读到对方工作目录或会话标识；检查模板不含模型密钥、ERP 凭据和正式会话数据；验证 guest 销毁/重建后的持久状态恢复。

当前证据：`cargo check`、`cargo test`（2 个现有 runtime 单元测试）和
`git diff --check` 通过；本地 Ubuntu 24.04 镜像已构建并启动容器，warmup
下 `/health/ready` 返回 200、`/health/live` 返回 200；真实配置缺少
`x-agent-runtime-token` 返回 401；启用测试延迟后第二个同时请求返回 409
`session_busy`。第一次用 Debian Bookworm 镜像的容器因 GLIBC 2.39/2.36
不匹配退出，已改用 Ubuntu 24.04 并记录该根因。真实 Cube v0.7.2 模板
`tpl-resident-20260925-v072b` 已 READY；合成案件的 JCode 首轮与续轮均完成，
ERP 两个只读工具成功，OpenViking 因服务未配置返回预期工具错误。
实例首个 turn 会绑定案件 `id`，后续不同案件请求返回
`session_scope_mismatch`，避免在同一常驻 Agent transcript 中串入另一案件。

### Phase 1：宿主 API 的 Cube 会话适配器（已接入）

- 在 `services/api` 增加案件会话到 Cube 实例的绑定/租约适配，不复制已有 Cube 生命周期逻辑。
- 定义内部 Bridge 协议：ready、turn、event、cancel、resume；所有请求携带短期、案件绑定、可撤销的 session token。
- Bridge 事件继续使用现有 `reasoning_delta`、`text_delta`、`tool_start`、`tool_done`、`compaction` 和终态事件，API 的持久化格式不改变。
- Phase 0 的 `/v1/turn` 先返回顺序稳定的 NDJSON 结果；API 适配器接通后再把这些事件实时转发到现有持久化/浏览器流，避免把 guest 内存队列当作正式历史。
- 为每个 active session 保存远端实例 ID、Bridge endpoint、租约版本和最后一次恢复检查点；远端信息不能取代 PostgreSQL 状态。

完成条件：API 能创建/恢复一个案件绑定实例，向 guest Bridge 发送一个真实短回合，并把消息、步骤和终态保存到现有表；重复请求不会创建第二个实例。

实现状态：`case_sandboxes.mcp_session_id` 保存案件级 MCP capability。当前 v0.7.2
模板不依赖 envd；实例创建后由后端通过已鉴权的 guest `/v1/configure` 下发案件绑定的
PostgreSQL session、MCP token、模型配置和 runtime token，再发送首轮请求。Worker
每轮发送 `new_session`（仅首轮）或按数据库 revision 恢复，并将 guest 事件按原有
Reply 协议写回 PostgreSQL。公开 sandbox 状态不返回内部 token/租约字段。

### Phase 2：Worker 切换执行后端（已接入，显式 opt-in）

- 抽象当前 `local-process` RuntimeExecutor，新增 `cube-resident` 实现；通过 `AGENT_EXECUTION_BACKEND=cube` 显式切换，禁止静默降级。
- Worker 取得案件锁后先确保 Cube/Bridge ready，再发送 turn；回合结束释放案件锁但保留活跃实例，空闲策略由 Cube 生命周期处理。
- 停止请求通过 Bridge cancel，再由 API 写入终态；断线时撤销 token、标记回合可恢复状态并重新校验 Cube 实例，不自动重复外部业务写入。
- 宿主机实现保留用于对照和回滚，不作为 Cube 验收结果。

完成条件：两个案件可并行运行；同一案件不会出现两个 active turn；API/Worker 重启后能恢复同一个 PostgreSQL session，并在新 Cube 实例中继续；权限撤销后旧 token 不能调用 Bridge。

实现状态：首条消息自动调用现有 Cube 生命周期创建接口并轮询 guest readiness，后续回合
只调用已绑定实例的 `/v1/turn`。每个案件仍受 PostgreSQL advisory lock 限制为单活；
每条消息不会创建新沙箱。首轮与 API/Worker 重启后的第二轮复用了同一 Cube 实例和
JCode PostgreSQL session；两个合成案件使用不同实例和会话。真实探针已覆盖同时运行、
销毁/重建恢复、旧 token 拒绝和同一案件的串行/幂等约束。

### 2026-09-25 故障定位、升级与真实回合

- 显式 Cube backend 的 MCP 验证在约 7 秒后返回 `Storage write outcome unknown; restart and reload before continuing`。对应 PostgreSQL JCode session 仍为 revision 0、metadata 为 null；首次持久化请求没有到达宿主 API。Bridge 和 guest health 正常。
- 创建请求已经按 Cube API 契约发送 `network.allow_out`。即使放行宿主 LAN IP、Cube 网关 IP 和模型域名，guest 对宿主 API 的 TCP 连接仍超时；这不是 HTTP 路径名、PostgreSQL 队列或模型响应造成的。
- TAP 抓包看到 guest 发出 SYN；已装 v0.4.0 的 CubeVS 把源地址 SNAT 为宿主 LAN IP 后，将目标为宿主自身 LAN IP 的包发向物理网卡，宿主监听进程收不到连接。Cube 官方 v0.5.0 更新记录明确增加“沙箱到宿主机自身 IP”的 BPF 快捷路径；本机版本早于该修复。
- 官方 v0.7.2 one-click 包的 SHA-256 为 `da5746dc277418363812aab91ed669cec56c411b77040ee26fb7c2be19d98e21`，与 Release asset digest 一致；本机现运行 v0.7.2，`cubemastercli --help` 与本地 API/guest 探针均已核对。旧模板不预设跨版本兼容，已注册新版 resident 模板。
- 当前 Cube 配置为 `CUBE_SANDBOX_MINIO_ENABLED=0`、`CUBE_ARTIFACT_STORE_BACKEND=fs`、`CUBE_OPS_STORE_BACKEND=fs`。Cube 默认 MinIO 用于 S3 兼容卷，也可作为模板产物/CubeOps 存储；本机这两类数据使用文件存储，不启动 MinIO。Forge 原件仍在独立 RustFS，Agent 会话仍在 PostgreSQL。若未来使用 Cube 的 S3 卷，须另配 MinIO 或外部 S3。
- v0.7.2 Cubelet 会重写非预签名 HTTP 模板产物地址到 CubeMaster 主机；本机用单节点镜像目录暂存 ext4 产物后注册 `tpl-resident-20260925-v072b`。这个操作只适用于当前单节点环境，多节点交付需单独设计。
- 首次真实首轮成功后，第二轮因 guest 只在首次构造 Agent 时发送 `skill_catalog`，而 Worker 对每个 `agent_runs` 行都要求先看到目录，触发 `Model activity before catalog`。Bridge 已改为每轮在案件绑定检查后发送同一技能目录；重建模板后，同一案件两轮均为 `completed`，两条运行记录具有相同目录哈希。
- 合成案件 `c4b35698-5bb7-4d64-8390-e16060c8814f` 的前两轮复用 Cube `96a105e74197412bb654dc0adf162b88` 与 PG session `session_69eb4b5ce9ed413399443d0ef632cf75`；第二轮在 API/8 个 Worker 重启后完成。另一合成案件 `6526b172-ee04-4317-b5fe-5c213a7a13da` 使用 Cube `1e21ff9a94f04d54b913d454cc7ecf2a` 与独立 PG session `session_34607b0e82464a51848aaf9b564428e5`，首轮完成。两案均完成 ERP 订单列表/明细只读调用；OpenViking 未配置，返回预期错误。此证据证明独立绑定，不等于完整跨案件越权或并发容量验收。
- `npm run check` 通过，包括 59 个 Rust API 单测、3 个 API 服务测试、系统集成测试及 Web 构建；Runtime Bridge 的 2 个单测通过。上述两个合成实例已通过业务 API 释放，Cube CLI 按各自 ID 查询均返回 0 个实例；Cube API 与 CubeOps health 正常。

### Phase 3：恢复、暂停和安全边界

- 验证 Cube pause/resume 后 Bridge 重连、JCode session 恢复和 MCP 连接重建。
- 验证 Cube 销毁/重建只恢复 PostgreSQL 已提交状态，不依赖沙箱内 JSON/JSONL 或快照中的业务凭据。
- guest 到宿主 API 只开放必要的内部路由；Cube 控制面不进入 Agent 工具目录。
- 记录模板版本、实例版本、session fencing 和恢复原因，便于处理 provider 超时和重复回执。

2026-09-25 真实探针：`/tmp/erp-cube-acceptance/lifecycle-1790307089530/report.json`
验证了释放原实例、创建新实例后沿用同一个 PostgreSQL JCode session 完成下一轮。
`/tmp/erp-cube-acceptance/1790275006368/report.json` 的 8 路冷/暖回合各完成
8/8，峰值并发 8；冷轮首 token P95 为 2,122 ms、总耗时 P95 为 11,826 ms，
暖轮分别为 1,016 ms 和 5,670 ms。这不是 50 用户容量验收。

暂停恢复探针连续失败：Cube 实例恢复为 `running`、Bridge 入口可达，
但 JCode 初始化时 `GET https://api.deepseek.com/models` 收到
`connection reset`；重建 resident Agent、对该幂等读取做有限重试和恢复后等待
10 秒均未消除故障。失败证据见
`/tmp/erp-cube-acceptance/lifecycle-1790308371282/report.json`；保留的合成实例
已通过业务 API 释放。故障不能归类为已修复，也不能把上述 8 路数据当作暂停恢复性能。

Cube v0.7.2 [生命周期文档](https://github.com/TencentCloud/CubeSandbox/blob/v0.7.2/docs/guide/lifecycle.md)
明确说明 guest 发起的出站 socket 在暂停时断开，应用须在恢复后重开连接。
同版本 [网络说明](https://github.com/TencentCloud/CubeSandbox/blob/v0.7.2/docs/blog/posts/2026-06-23-cubesandbox-network-deep-dive.md)
说明域名 `allow_out` 依赖 guest 的 UDP DNS 查询来学习临时放行 IP；
[出网策略指南](https://docs.cubesandbox.com/zh/guide/network-policy)
说明策略拒绝和缺失出站 session 都可能向 guest 返回 TCP RST。
2026-09-25 再核查：当前 `v072d` 模板的立即恢复探针仍在
`GET https://api.deepseek.com/models` 收到 `connection reset`；将恢复后第二轮
推迟 100 秒，仍失败，但 guest stderr 变为 `Temporary failure in name resolution`
（`/tmp/erp-cube-acceptance/lifecycle-1790313168033/report.json`，对应实例已释放）。
同一时刻宿主机解析该域名成功，未认证 HTTPS 探针返回 401。由此不能把故障
单独归因于 JCode 的共享 HTTP 连接池；能确认的是恢复后 resident 进程内
的解析/连接请求失败，尚未证明 guest 内新进程也会失败，更未区分解析器、
DNS 放行规则和 CubeVS 会话状态。Cube Master 恢复代码会
从保存的规格重新填充网络配置，故也不能断言 `allow_out` 必然丢失。
官方 Pi 暂停恢复示例逐轮运行新的 `pi --print` 进程，且默认开放出网，
并非本项目常驻 JCode + 限域名出网策略的同条件对照。旧基础模板虽然标记
`READY`，创建实例却因根文件系统快照缺失失败，未能提供新进程对照。
同 `v072d` 模板另建不带密钥的实例，暂停前后均能创建/恢复，但尝试通过
E2B SDK 运行 `getent`/`curl` 均在命令执行接口返回 `Code.unknown`；resident
模板没有 envd sidecar，因此这次尝试没有测到 DNS，实例已释放。
后续使用 CubeMaster 容器 exec 在相同 `v072d` 模板中运行**新进程**作对照，
均不注入模型密钥：新建实例的 `getent api.deepseek.com` 和 443 TCP 均成功；
先 pause/resume 再执行同样命令，DNS 与域名 TCP 均超时。为 DeepSeek 的
当前 IPv4 地址显式添加静态 `/32` 后，恢复实例的直连 443 成功而 DNS 仍超时，
因此不是 JCode 常驻连接池或整个出站路径的问题。恢复后按原值调用 Cube
`update_network`，DNS 仍超时；再将 guest `/etc/resolv.conf` 的 nameserver
`169.254.254.53/32` 加入完整 `allow_out`，DNS 立即成功。另一个实例从创建时
就显式包含该 resolver `/32`，pause/resume 后新进程 DNS 和域名 443 均成功。
每个临时实例均已释放，Cube `/health` 回到 0 实例。

这定位了本机 Cube v0.7.2 受限出网策略在恢复时未保住 DNS 解析器的
出站例外；还未通过 eBPF map/抓包确认例外究竟在哪一步丢失。
Forge 创建实例时显式添加 `CUBE_DNS_RESOLVER` 的 IPv4 `/32`，恢复旧实例时
重设包含该规则的完整受限策略。该规则必须匹配 guest 的实际 nameserver；
若 Cube 部署修改了 DNS 地址，须同步配置。该 DNS 规避先通过无密钥沙箱
网络探针，后由下述 JCode 真实回合验证；这不代表其他恢复场景已验收。
CubeMaster 的内部 exec 会留下任务，使同一实例拒绝暂停，所以诊断先暂停
再 exec，并未把该内部接口加入业务 Agent。

同一受限出网策略下，加入 DNS 规则后，新进程解析与 443 连接均正常，
但旧模板的 resident JCode 在恢复后仍于 `/models` 收到 `connection reset`。
Bridge 的 `/v1/reconnect` 仅丢弃 `ResidentRuntime`，JCode 的进程级共享 HTTP
客户端仍保留暂停前连接。Bridge 初始化新 Agent 时改用 JCode 提供的
`fresh_transport_client` 获取模型目录，不修改子模块；业务 Responses 请求
仍由 JCode Provider 负责。新 OCI 模板 `tpl-00f062671d894be6808685fa` 在本机
Cube v0.7.2 READY，本地开发 API/Worker 已切到该模板。

真实合成探针 `/tmp/erp-cube-acceptance/lifecycle-1790316643961/report.json` 和
`/tmp/erp-cube-acceptance/lifecycle-1790316867304/report.json` 均无错误完成：
首轮调用 ERP 只读工具；同一 Cube 实例暂停恢复后的续轮完成且
没有重复调用工具；销毁后新实例沿用同一 PostgreSQL session 并完成第三轮。
新增的三次暂停恢复探针
`/tmp/erp-cube-acceptance/lifecycle-1790318427060/report.json` 仍复用同一
operation、sandbox 和 PG session；同案件三次续轮全部完成，重复 client message
只产生一次保存消息，另一个同时提交的新请求返回 409 `agent_busy`。
第一轮的保存回复均有正文，所有 case_sandboxes 记录为 `released`；脚本结束后
Cube `/health` 均返回 0 个实例。写操作断线、跨节点迁移仍不在本轮范围内。
失败模板的 OCI 导入尝试只作为构建诊断，
不是运行时故障证据。

### Phase 4：同口径性能和容量验收

固定模型、Prompt、工具集合、输入、CPU/内存和数据库条件，至少比较：

1. 宿主机常驻基线。
2. Cube 暖实例。
3. Cube 干净模板创建后首回合。
4. Cube 暂停恢复后的首回合。

每组记录队列等待、Cube 创建/恢复、Bridge ready、首 reasoning、首正文 token、工具耗时、总耗时、错误率、CPU/RSS、实例清理和跨案件串扰。没有这些分段数据，不把单一 P95 归因于 Worker、Cube 或模型。

2026-09-25 真实 Cube/JCode 容量探针：8 个 worker 已确认每个读取
`AGENT_MAX_CONCURRENT_RUNS=8`，总调度槽位为 64。此前 50 用户探针只有 32/50
完成，18 个回合在 60 秒 ready 窗口内失败；当时 8 个 transient unit 错误地只设置
了 `AGENT_WORKER_MAX_CONCURRENT_RUNS=8`，Rust 因此回退到每进程默认 4。修正用户级
worker 环境并重启后，`/tmp/erp-cube-acceptance/1790319935198/report.json`
的冷、暖两轮均为 50/50，峰值并发 50，错误率 0，所有 50 个实例均清理完成。

| 路径 | 队列 P95 | 首 token P50/P95 | 总耗时 P50/P95 |
| --- | ---: | ---: | ---: |
| Cube 冷启动 | 242 ms | 4,123 / 6,296 ms | 34,655 / 36,787 ms |
| Cube 暖实例 | 77 ms | 1,438 / 2,228 ms | 16,505 / 19,250 ms |

两轮均通过跨案件 runtime token 校验（正确 token 200，错误 case token 401），
`isolated=true`，结束时 Cube health 为 0 实例。8 用户回归的冷/暖首 token P95
分别为 2,373/1,464 ms；50 用户结果说明当前本地配置可以同时接纳 50 个独立案件，
但不构成模型服务的长期吞吐或生产 SLA 承诺。性能归因仍需拆出 DeepSeek、Bridge、
工具和 Cube 创建时间，并继续观察 PostgreSQL 连接、内存和节点资源。

## 关键接口约束

- `GET /health/ready`：进程和配置可接收任务后成功；响应中的 `agent_initialized` 必须为 true 才表示该实例已经完成案件绑定、JCode、PostgreSQL session 和受限 MCP 初始化。
- `POST /v1/turn`：提交一个已持久化的 `message_id`，服务端按 fencing 版本拒绝重复或过期请求。
- `GET /v1/events` 或等价流：返回有序运行事件；断线后以 PostgreSQL checkpoint 恢复，不依赖内存重放。
- `POST /v1/cancel`：只取消当前案件回合，不修改外部 ERP 状态。
- 所有 guest 请求都必须带短期 session token、tenant、user、case 和 session 绑定；Bridge 不接受浏览器直连。

## 风险与回滚

- Cube 模板无法稳定启动 JCode：保持宿主机后端，记录为模板阻塞，不伪造 Cube 结果。
- guest 到宿主 API 路由不满足最小权限：先做本地回环和受控 fixture，禁止把 API 绑定到公网或放宽为任意 Bash/任意 HTTP。
- 暂停恢复丢失外部连接：以 PostgreSQL 事件和 fencing 为准，重建连接后重新读取 ERP 状态；不自动重放写操作。
- Cube 资源不足：限制活跃实例和队列并发，保留宿主机对照；不把基础创建并发数字当作业务容量。

## 验收证据

- 模板和 Bridge readiness：版本化启动日志、两个实例隔离探针、无凭据镜像检查。
- API/Worker：现有 `npm run check` 加 Cube fixture、重复提交、权限撤销、停止、重启和恢复测试。
- 真实本地环境：至少两个案件的暖运行、暂停恢复和清理记录；所有消息/工具/压缩状态在 PostgreSQL 中可核对。
- 性能报告：四种路径的原始样本、分段 P50/P95/P99、错误率和资源数据；明确模型波动与 Cube 开销的边界。

当前下一步：补齐写操作断线保护、跨节点迁移和长期混合负载；为 worker transient
启动方式补充可重复的源配置，避免只依赖当前用户级 manager 环境。随后做宿主机/Cube
同口径性能对照，并接着验收 OpenViking 检索、文档解析和 ERP 写入边界。当前 Cube
backend 已可完成本地真实合成只读回合；OpenViking 检索和 ERP 写入不在此次容量验收内。
