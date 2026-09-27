# Cube 常驻 JCode 运行时

状态：本机 Cube 已升级至 v0.7.2；Phase 0/1/2 的常驻 JCode 路径已在真实 Cube guest 完成首轮、同案件续轮和 API/Worker 重启后续轮验证。两个合成案件使用不同实例与 PostgreSQL 会话；销毁重建、同一实例三次暂停恢复、同案件串行/幂等和清理后的 50 用户冷/暖并发均已通过。另有一轮突发创建出现 7/50 个 Cube HTTP 500，现已修复业务侧 60 秒无效等待，但 Cube 侧瞬时错误仍需重复压测定位，不能据此宣称容量或生产 SLA。8 个独立 worker 每个 8 槽，总计 64 个调度槽位；此前 50 用户测试只完成 32 个的根因是启动单元使用了错误的 `AGENT_WORKER_MAX_CONCURRENT_RUNS` 名称，Rust 实际读取的 `AGENT_MAX_CONCURRENT_RUNS` 已修正。宿主机 Harness 保留作回归基线。本文只描述 Agent 运行路径，不改变 ERP/OpenViking、RustFS 或文档解析的服务边界。

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
8/8，峰值并发 8；冷轮首段正文 P95 为 2,122 ms、总耗时 P95 为 11,826 ms，
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

| 路径 | 队列 P95 | 首段正文 P50/P95 | 总耗时 P50/P95 |
| --- | ---: | ---: | ---: |
| Cube 冷启动 | 242 ms | 4,123 / 6,296 ms | 34,655 / 36,787 ms |
| Cube 暖实例 | 77 ms | 1,438 / 2,228 ms | 16,505 / 19,250 ms |

两轮均通过跨案件 runtime token 校验（正确 token 200，错误 case token 401），
`isolated=true`，结束时 Cube health 为 0 实例。8 用户回归的冷/暖首段正文 P95
分别为 2,373/1,464 ms；50 用户结果说明当前本地配置可以同时接纳 50 个独立案件，
但不构成模型服务的长期吞吐或生产 SLA 承诺。性能归因仍需拆出 DeepSeek、Bridge、
工具和 Cube 创建时间，并继续观察 PostgreSQL 连接、内存和节点资源。

### 2026-09-25 Cube v0.7.2 性能复核与下一轮实验

- 上表两轮不是同口径 A/B：冷轮 46/50 次调用 2 个工具，3 次调用 3 个、1 次调用 4 个；暖轮 50/50 次没有工具调用。因此总耗时 P95 的差额不能直接归因于 Cube 创建。`agent_runs.first_token_at` 只在首个 `text_delta` 或 `text_replace` 时记录，是从运行开始到首段正文的等待，不是首个 reasoning token，也不是纯模型 TTFT；它可能包含此前的创建、初始化与工具工作。原始数据在 `/tmp/erp-cube-acceptance/1790319935198/report.json`。
- 当前模板的 `CUBE_TEMPLATE_WARMUP=1` 只检查 Bridge 可执行文件和 HTTP listener；案件密钥、PostgreSQL session、MCP capability 与 JCode Agent 在实例创建后配置、首轮初始化。Cube 官方[模板预热指南](https://github.com/TencentCloud/CubeSandbox/blob/v0.7.2/docs/guide/tutorials/prewarm-template-service.md)要求探针在真正可复用的初始化完成后才返回成功，并要求模板无密钥、外部连接恢复后重建。先测首轮 JCode 初始化占比，再考虑把无密钥、无租户状态的静态初始化移入模板；不把案件 session 或凭据快照进去。
- 当前创建请求设置 `timeout=900`，未设置 `lifecycle.on_timeout`；按 Cube [生命周期说明](https://github.com/TencentCloud/CubeSandbox/blob/v0.7.2/docs/guide/lifecycle.md)，默认超时行为是 `kill`，并非自动暂停。已验证的暂停恢复是受控探针，不代表自动暂停策略已启用。若目标是保留会话并回收空闲 CPU/内存，应单独实验 `on_timeout=pause` 与 `auto_resume`，记录恢复延迟、重连失败率、快照占用和空闲成本；恢复后继续重建外部连接。
- Cube [资源指标](https://github.com/TencentCloud/CubeSandbox/blob/v0.7.2/docs/guide/resource-metrics.md)可从 Cubelet 的 `/v1/metrics/resource` 读取宿主侧和 guest 工作负载的 CPU、限流与内存；当前空实例时没有系列是正常的。端口没有内建鉴权/TLS，只在受信管理网络采集。Cube [调度文档](https://github.com/TencentCloud/CubeSandbox/blob/v0.7.2/docs/guide/cubemaster-scheduler-config.md)说明 `host.quota.*=0` 使用推导默认值，不等于无限容量；在提高创建并发或增加节点前先采集有效 MVM、CPU、内存、磁盘及创建配额。`paused_resource_release_ratio=0` 保留暂停实例的调度配额，确保恢复容量；提高它会增加密度，但满载时恢复可能返回 409，需按恢复成功率与成本取舍。

下一轮按相同模型、思考程度、输入、输出长度和工具次数做成对对照，分别跑宿主机、Cube 干净创建、同实例暖轮、暂停恢复；每条路径测 1/8/32/50 并发并重复采样。先打点提交、领取、Cube create/ready/configure、JCode init、`/models`、模型首 reasoning/正文、各工具、完成及清理，再同时采集 PostgreSQL 连接、Cubelet 指标与宿主机 CPU/内存/磁盘。以分段 P50/P95/P99、错误率、工具正确性和资源成本选择优化目标；只有 Cube create/init 占比显著时才优先改模板预热或轮询，模型/工具占主导时先处理相应路径。当前 8×8 worker 已让 50 路峰值进入运行态，继续加槽位不能替代瓶颈归因。

### 2026-09-25 分段打点与新模板实测

`agent_runs.timings` 现按每次执行尝试保存版本化分段样本：Worker 领取后到
Cube ready、输入准备、Bridge 请求/目录、首 reasoning/正文、各工具、完成；
Cube 创建请求、guest 配置和 JCode 构建/模型目录耗时分别记录。采样不含密钥和
消息正文。`/tmp/erp-cube-acceptance/stage-timing-one/report.json` 验证新模板
`tpl-4e16d15a916043b7b221ec7f` 的首轮初始化为 209 ms、续轮复用为 0 ms；
首轮和续轮均完成并释放实例。该本地模板现由 `.runtime/cube.env` 选用。

| 同一短提示、50 用户 | 冷实例 P95 | 暖实例 P95 |
| --- | ---: | ---: |
| 排队 | 200 ms | 52 ms |
| Cube 实例就绪 | 2,394 ms | 489 ms |
| 其中创建并配置 | 1,517 ms | 不适用 |
| Bridge 请求到目录 | 730 ms | 9 ms |
| 其中 JCode 构建 | 722 ms | 0 ms |
| 目录到首段正文 | 1,252 ms | 1,160 ms |
| 提交到首段正文 | 4,161 ms | 1,644 ms |
| 提交到完成 | 4,232 ms | 1,708 ms |

原始 50 用户样本：`/tmp/erp-cube-acceptance/stage-timing-fifty/report.json`。
冷、暖各 50/50 完成，冷轮峰值运行 50，暖轮峰值 44；两轮均未调用工具。
`reused` 标记分别为 50 次 false 和 50 次 true；跨案件 token 返回 401，
50 个实例均已释放，Cube 最终为 0 实例。8 用户真实 ERP 只读回合的样本在
`/tmp/erp-cube-acceptance/stage-timing-eight/report.json`：冷、暖各 8/8；
冷轮每个会话均完成订单列表和明细读取，冷轮首段正文 P95 为 2,411 ms、
总耗时 P95 为 12,415 ms、JCode 构建 P95 为 301 ms、工具耗时合计 P95 为
731 ms；暖轮不重复调用工具，总耗时 P95 为 6,011 ms。

这些分段有包含关系，不能相加：实例就绪包含创建/状态轮询，Bridge 到目录
包含 JCode 构建，目录到正文仍包含模型流与事件传输。`first_token_at` 是保存
首段正文的时间，并非提供商原始 TTFT。短提示冷轮主要观察到 Cube 就绪和
JCode 初始化成本；ERP 多工具回合的后续生成时间更长，但现有打点不能把
它完全归因于模型。

### 2026-09-25 宿主机对照、暂停恢复与资源采样

`scripts/verify-cube-resident-live.mjs` 可用 `VERIFY_RUNTIME_BACKEND=host|cube`
跑相同探针；宿主机路径是**每回合新建 JCode 进程**，不是常驻宿主机 Harness。
短提示“请只回复‘收到’，不要调用工具。”在两条路径各跑了两次 50 用户冷/暖轮，
每轮 50/50 完成且无工具调用、跨案件会话隔离成立。下表为提交到首段正文的 P95；
同一提示的暖轮已有前文，因此上下文与生成长度并不完全相同。

| 50 用户短提示 | 第一次冷/暖 P95 | 复测冷/暖 P95 | 复测队列冷/暖 P95 |
| --- | ---: | ---: | ---: |
| Cube 常驻实例 | 4,528 / 1,722 ms | 4,014 / 1,691 ms | 104 / 62 ms |
| 宿主机每回合进程 | 4,795 / 2,177 ms | 5,001 / 2,031 ms | 369 / 164 ms |

复测的 Cube 冷轮实例就绪 P95 为 2,269 ms、JCode 构建 P95 为 664 ms，
暖轮分别为 414/0 ms；宿主机进程启动到工具目录 P95 为冷 171、暖 138 ms。
这两条路径的模型服务、网络、系统负载与初始化形态并不等价，不能据短时差额
宣称 Cube 固有更快或推断提供商 TTFT。原始报告分别在
`/tmp/erp-cube-acceptance/matched-{cube,host}-fifty{,-repeat}/report.json`。

真实 ERP 只读探针又在每条路径跑了 8 用户冷/暖轮，均为 8/8 完成。
冷轮全部完成 `mcp__erp__list_erp_orders` 和 `mcp__erp__read_erp_order`；
Cube 一回合额外加载了 `skill_manage`，暖轮均未再调用工具。Cube 冷/暖
首段正文 P95 为 4,181/1,390 ms、总耗时 P95 为 12,525/6,933 ms；宿主机
对应为 6,460/1,565 ms 和 15,455/6,173 ms。复述文本与工具结果已抽样
核对，但两组输出长度不同，不能直接当作相同工作量的模型吞吐对照。
报告：`/tmp/erp-cube-acceptance/erp-read-{cube,host}-eight/report.json`。
探针现在会断言 ERP 冷轮的两种工具都完成、暖轮不重复查 ERP；短提示可设置
`VERIFY_EXPECT_NO_TOOLS=1` 强制检查零工具。

Cube 50 用户复测时，每 500 ms 采样一次 `/proc`、进程 RSS、PostgreSQL 连接
和 Cubelet `/v1/metrics/resource`：捕捉到最多 50 个沙箱指标系列，宿主侧沙箱
cgroup 内存总值最高 487 MiB，API 进程合计 RSS 最高 272 MiB，数据库连接数
最高 151。宿主机每回合进程对照捕捉到最多 52 个 runtime 进程（含 2 个原有
进程）、合计 RSS 最高 1,728 MiB，API RSS 最高 242 MiB，连接数最高 91。
各轮开始前连接基线分别约 143 与 34；不同时间、不同进程组合和 Cubelet
指标口径使上述内存数字不可直接相减。主机 CPU 曾达 97%，但 Cube 那次峰值
发生在沙箱系列为 0 的采样点，无法归因。采样原件：
`/tmp/erp-cube-acceptance/resources-{cube,host}-fifty.json`。采样脚本为
`scripts/sample-agent-runtime-resources.mjs`，这些是单节点短时观察，不是容量
或成本基准。

同一短提示的 Cube 生命周期探针还验证了暂停恢复：首轮、暂停恢复续轮、
释放重建续轮均完成；恢复后保留同一案件、operation 和 PostgreSQL session，
但 resident 进程被重建（`reused=false`）。三轮提交到首段正文分别为
1,913、1,756、1,515 ms；样本各 1 次，不能用来估算 P95。原件在
`/tmp/erp-cube-acceptance/matched-lifecycle-one/report.json`。

下一步先测提供商请求发出至首 reasoning/正文事件、重复跑更长的 ERP/知识
混合负载，并区分空闲池连接与负载新增连接；只有确认耗时落在 Cube 创建、
JCode 初始化或模型服务的哪一段后，再调整预热、实例调度或 Worker 槽位。

本地旧 transient worker 单元仍带误拼的
`AGENT_WORKER_MAX_CONCURRENT_RUNS=8`，但 `scripts/start-dev-workers.mjs` 将
正确的 `AGENT_MAX_CONCURRENT_RUNS=8` 注入用户级 manager，实际进程同时继承
两项。本轮从 `/proc` 验证了正确变量；若改用其他启动方式，必须在进程环境
明确设置正确名称，不能据旧单元展示值推断槽位。

### 2026-09-25 创建突发错误与恢复修复

在干净 Cube 状态、worker 池为 12 的首轮 50 路短提示探针中，43 个请求完成，
7 个请求由 Cube `POST /sandboxes` 在 6--13 ms 内返回 HTTP 500。开始采样时
Cube 实例数为 0；失败记录没有 `sandbox_id`，数据库连接峰值约 123，不能据此
归因于 PostgreSQL 池耗尽或已有实例占位。随后把池临时设为 16 的干净冷/暖轮为
50/50，池恢复为 12 后再次冷/暖轮也为 50/50；直接绕过业务 API 的 50 路创建探针
收到 50 个 HTTP 201，并清理了全部实例。对应业务压测报告分别为
`/tmp/erp-cube-acceptance/matched-cube-pool16-control/report.json`、
`/tmp/erp-cube-acceptance/matched-cube-pool12-repeat/report.json`；独立创建探针
没有保留 JSON 文件，结果以当次终端输出为准。
随后检查 `/data/log/CubeMaster/cubemaster-req.log`，18:20:01--02 的七次失败均为
CubeMaster `RetCode=130597`、`no more resource`。其中一条失败请求的调度日志显示
`quotaCpuFree=-38000m`、请求 `cpuq=2000m`，以及 `quotaMemFree=-6348Mi`、请求
`memq=2000Mi`，CPU/内存过滤后均无可选节点。当时只有一台健康节点，其可调度额度
为 48000m CPU、79652 MiB 内存；原模板每个实例申报 2000m/2000Mi。这是 Cube
调度配额耗尽，不是 PostgreSQL 队列或模型端返回的 500。突发创建中的资源统计曾出现
负的剩余额度，故不能用事后实例数量反推每一刻的实际并发占用。

此前业务路径把创建错误交给 `reconcile`，错误被保存为
`creation_unconfirmed`/`cube_unavailable`，随后 `ensure_resident_ready` 仍把没有
实例 ID 的 `creating` 记录按正常创建继续轮询，直到 60 秒 ready deadline，前端才看到
“Could not finish this reply”。当前实现先按唯一 operation metadata 做一次对账；
找不到实例就立即返回 `cube_runtime_create_unconfirmed`，保留记录供下一回合对账，
不盲目发起第二次创建。若 Cube 在错误响应前实际创建了实例，对账仍会绑定它并继续配置，
避免重复沙箱。Provider 只记录 HTTP 方法、路径和状态码，不记录响应体、令牌或业务数据。

`cargo fmt --manifest-path services/api/Cargo.toml -- --check` 与完整 `npm run check`
均通过；现有 Cube fixture 还覆盖了丢失创建响应、对账恢复和不重复创建。

### 2026-09-25 资源声明调整与容量复测

Cube `create-from-image` 未指定资源时默认申报 2000m CPU、2000 MiB。用相同的
`resident-20260925-timing` 镜像另建本地模板
`tpl-22e9c0c9f7724f26998b0a33`，显式申报 750m CPU、1024 MiB，READY 副本
位于健康节点。这是按现有轻量 Agent 工作负载测试的调度声明，不是对长上下文、图片解析
或未来 RAG 的资源保证。`.runtime/cube.env` 已选用新模板，API 与八个 Worker 重启后，
进程环境均核对为新 ID。此前 systemd 用户环境中残留更旧的模板 ID，且配置文件变更并不
会自动改变已运行进程；旧报告因此不能只按文件内容归属模板。验收脚本现在从 Cube
实例清单核对每个实际 `templateID`，并记录 CPU/内存规格。

| 新模板本机探针 | 冷/暖完成 | 首段正文 P95 冷/暖 | 其他验证 |
| --- | --- | --- | --- |
| 8 用户 ERP 只读 | 8/8、8/8 | 3,946 / 1,425 ms | 冷轮调用 ERP 工具，暖轮无工具 |
| 32 用户短提示 | 32/32、32/32 | 3,270 / 1,511 ms | 模板与实例隔离通过 |
| 50 用户短提示复测 1 | 50/50、50/50 | 4,080 / 1,596 ms | 50 个模板 ID 逐一核对 |
| 50 用户短提示复测 2 | 50/50、50/50 | 7,973 / 1,620 ms | 50 个模板 ID 逐一核对 |
| 50 用户短提示资源采样轮 | 50/50、50/50 | 7,959 / 1,781 ms | 50 个模板 ID 逐一核对 |

还有一次切换后、模板校验脚本修改前的 50/50 冷暖轮，首段正文 P95 为
8,248/2,422 ms。带校验的两次 50 路复测均拒绝跨案件 token（401），各自清理
50 个实例且无清理错误；采样轮结束后 Cube 实例数也为 0。新模板测试期间 CubeMaster
未再记录 `130597`。原始报告位于 `/tmp/erp-cube-acceptance/tuned-750m-*`。

50 路采样轮的 Cubelet 指标最多出现 50 个沙箱系列，宿主侧沙箱 cgroup 内存合计峰值
408 MiB（并非 guest 工作集），宿主可用内存最低约 44.9 GiB，PostgreSQL 连接峰值
119，宿主 CPU 忙碌度瞬时峰值 94.5%。这轮 JCode build/model catalog 的 P95
分别为 5,259/5,115 ms，创建及配置的 P95 为 1,104 ms；不能把冷轮尾延迟都归因
于 Cube 创建或远端模型。32 路采样的模型目录 P95 为 144 ms，但两组不是同一时刻的
受控 A/B。下一步应对 32/50 路交替重复采样，并按初始化、模型与宿主 CPU 同步分析。

Cube 官方[调度配置指南](https://github.com/TencentCloud/CubeSandbox/blob/v0.7.2/docs/guide/cubemaster-scheduler-config.md)
要求核对有效 CPU、内存、MVM 和创建并发额度；`0` 表示按宿主推导默认值而非无限容量，
并建议至少两台计算节点用于节点选择和故障隔离。本地目前只有一台健康节点；50 路短
提示通过不代表 64 个 Worker 槽位、长上下文或混合解析负载都已验收。保持实例资源
声明与实际工作负载匹配，继续测长会话和工具负载，再决定入口容量限制或增加节点。

#### 2026-09-25 swap 与多轮上下文探针

检查宿主机时，swap 总量为 64 GiB、已用约 25.8 GiB；`MemAvailable` 约 45 GiB，
memory PSI 的 `some/full` 近 10 秒均为 0。没有据此扩大 swap：已占用 swap 本身不表示
当前内存不足，扩容也不会增加 Cube 模板的调度内存额度。

用专用 750m CPU / 1 GiB 模板执行了两组各 8 个独立案件的合成输入。消息接口上限为
4,000 字符，初次 16 KiB 单消息样本在入队前被业务 API 正确拒绝，没有启动 Cube；之后
改为每轮 3.27 KiB、同实例连续 6 轮，48/48 回合完成，未调用工具，答案校验正确，案件
会话隔离成立，全部 8 个 Cube 实例已释放。冷轮首段正文 P95 两组分别为 2,258 ms 和
6,841 ms；后者 JCode build 与 model catalog P95 分别为 5,219 ms 和 5,123 ms。暖轮
首段正文 P95 为 964–1,586 ms。长会话样本没有出现随轮次增加的延迟递增趋势，但该
合成语料仅累计约 20 KiB 原始输入，不等于大上下文容量验收。

并行 500 ms 采样窗口最低可用内存约 44.7 GiB，swap 使用保持约 25.8 GiB，swap-out
增量 0 页、swap-in 增量 43 页，memory PSI 保持 0，未见 swap 压力迹象。Cubelet 最多
导出 8 条 `host_sandbox` 系列、合计 82 MiB、0 次内存限额失败；它没有导出
`guest_workload` 系列，所以 82 MiB 不是 guest 工作集，不能用于判断 1 GiB 模板的
真实余量。Cube 官方[资源指标指南](https://github.com/TencentCloud/CubeSandbox/blob/v0.7.2/docs/guide/resource-metrics.md)
说明默认只采集 `host_sandbox`；测工作负载内存需设 `export_scopes=["guest_workload"]`
或 `all`、重启 Cubelet，并确认模板内 `cube-agent` 支持指标 capability v1 和统一
cgroup v2。当前执行环境禁止 sudo/root 操作，因此此配置尚未启用。下一步先取得受控
Cubelet 配置变更窗口并完成 guest 指标采集，再做更高并发、长会话及图片/解析混合负载；
在此之前不以扩大 swap 或提高每实例内存作为已证实的优化。

### 2026-09-25 Cube guest 身份与临时 Bash 探针复核

旧的本地探针模板 `tpl-c5191b1c75f84c7e9ec06538` 曾在真实 Cube 中完成冷、暖各
一轮五条固定只读命令，原始记录为
`/tmp/erp-cube-acceptance/bash-probe-live-20260925/report.json`，PostgreSQL
`agent_runs` 的两条消息（2090、2091）各保存五个 `bash:completed` 事件，JCode
`agent_session_entries` 保存了工具原始输出。`pwd` 是 `/home/forge/work`，但
`id` 返回 `uid=0(root) gid=0(root)`。相同镜像的 OCI 配置声明 `USER forge`，
直接 Docker 运行返回 UID 10001；两者不一致，不能以镜像声明代替 guest 验收。

`infra/cube/agent-template/resident-start.sh` 现于 root 启动时用 `setpriv`
降为 UID/GID 10001，再执行 Bridge；已是 10001 时直接执行，其他 UID 拒绝启动。
本地 Docker 模拟 root 启动的 PID 1 UID/GID 均为 10001。新镜像
`forge-agent-runtime:resident-nonroot-20260925` 导入 Cube 模板
`tpl-2c26a7913b524a53a024145d`（750m CPU、1024 MiB，READY）后，用以下命令
做真实冷/暖回合：

```sh
VERIFY_CUBE_COUNT=1 \
VERIFY_CUBE_OUTPUT=/tmp/erp-cube-acceptance/nonroot-probe-live-20260925 \
VERIFY_CUBE_MATCHED_PROMPT='请调用受限 bash 工具，分别执行 pwd、id、uname -a、cat /etc/os-release、cat /proc/1/cgroup。只报告每个工具的真实输出，不猜测环境。' \
node --env-file=.env --env-file=.runtime/cube.env scripts/verify-cube-resident-live.mjs
```

报告 `report.json` 的冷/暖均为 1/1 完成、各五个 `bash:completed`，无错误；
JCode PostgreSQL 原始工具结果两轮均为 `uid=10001(forge) gid=10001(forge)`，
`pwd=/home/forge/work`，`/etc/os-release` 为 Ubuntu 24.04.5，cgroup 包含
新模板 ID。冷/暖均使用同一个 Cube sandbox 和 PostgreSQL session，后者复用
runtime；结束时案件状态为 `released`，`cubemastercli list --all` 显示 0 个实例。
单样本时延不作为 P95 或容量结论。

随后把本地 `.runtime/cube.env` 中 `AGENT_TEMP_BASH_PROBE` 设为 `0`，重启
API 和八个 Worker，在同一新模板上用 `VERIFY_EXPECT_NO_TOOLS=1` 再跑一冷一暖
回合：

```sh
VERIFY_CUBE_COUNT=1 \
VERIFY_CUBE_OUTPUT=/tmp/erp-cube-acceptance/bash-disabled-live-20260925 \
VERIFY_EXPECT_NO_TOOLS=1 \
VERIFY_CUBE_MATCHED_PROMPT='请调用 bash 工具执行 id；如果没有这个工具，请明确说无法调用，不要编造命令输出。' \
node --env-file=.env --env-file=.runtime/cube.env scripts/verify-cube-resident-live.mjs
```

`/tmp/erp-cube-acceptance/bash-disabled-live-20260925/report.json`
记录两轮各 1/1 完成、工具调用数均为 0；模型明确报告当前无 Bash 工具，案件已释放，
Cube 实例数为 0。该次验证结束时本地运行配置保持开关关闭、模板为新 ID，API `/ready`
和九个服务均正常。临时探针只接受上述五条固定命令，代码单测拒绝任意命令；
`cargo fmt --check`（API、Bridge）、Bridge 5 项单测、完整 `npm run check`
和 `git diff --check` 均通过。这不等于通用 shell 获准，也不等于 ERP 写入、
跨节点或生产隔离验收。

### 2026-09-26 常驻会话跨回合模型切换

真实 Web 测试发现常驻会话第二轮从 Pro 改选 Flash 时，JCode 仅更新思考程度，
`agent_sessions.metadata.model` 仍记录 Pro。Worker 现在在本地和 Cube 回合请求中传入
模型 ID，Bridge 限定允许的模型并在常驻 JCode 上调用 `set_model`，同步 provider、
会话元数据与后续上下文窗口。新版镜像
`127.0.0.1:5000/forge-agent-runtime:model-turn-20260926` 对应模板
`tpl-02d04202aa634f329df8d19f`，单节点 READY，750m CPU / 1024 MiB。
本地 API 与八个 Worker 重启后，在真实 Cube/DeepSeek 回合中先用 Pro/关闭思考，
再用 Flash/最高；第二轮出现 reasoning 事件后主动停止，PostgreSQL 会话元数据
为 `deepseek-flash` / `max`，停止状态刷新后保留。命令为
`PLAYWRIGHT_MODULE=/tmp/erp-agent-flow-check/node_modules/playwright/index.mjs VERIFY_CONTROLS_OUTPUT=/tmp/erp-agent-controls-20260926 node --env-file=.env scripts/verify-web-agent-controls.mjs`，
报告和 20 张截图在 `/tmp/erp-agent-controls-20260926/`。Bridge 6 项单测和完整
`npm run check` 通过。本次只覆盖单会话本地路径，不用来推断混合负载或生产容量。

### 2026-09-26 当前模板 50 会话冷/暖探针

使用 `tpl-02d04202aa634f329df8d19f`（750m CPU / 1024 MiB）及八个
Worker 各八槽，运行以下相同的合成短提示：

```sh
VERIFY_CUBE_COUNT=50 VERIFY_EXPECT_NO_TOOLS=1 \
VERIFY_CUBE_OUTPUT=/tmp/erp-cube-acceptance/model-turn-50-20260926 \
VERIFY_CUBE_MATCHED_PROMPT='这是合成测试数据。请只回答 17 + 306 的结果；不要调用工具。' \
node --env-file=.env --env-file=.runtime/cube.env scripts/verify-cube-resident-live.mjs
```

`/tmp/erp-cube-acceptance/model-turn-50-20260926/report.json` 记录两轮各
50/50 完成、峰值同时运行 50、100 次均为真实 Cube backend 且无工具调用；
50 个实例均核对为目标模板，案件、PostgreSQL session 和实例 ID 各自独立，
错误案件 token 被 Bridge 以 401 拒绝。冷轮排队/首字/总耗时 P95 分别为
458/4,447/4,524 ms，暖轮为 49/1,912/2,100 ms。冷轮沙箱就绪 P95 为
2,071 ms，JCode build P95 为 730 ms；暖轮复用运行时，build 为 0。
50 个测试实例均已释放，Cube CLI 最后只剩先前浏览器验收的 1 个实例。

补查 PostgreSQL 原始消息发现回复正确率低于完成率：冷轮 46/50、暖轮
47/50 包含正确答案 `323`；其余 7 条实际回复为 `341`、`413` 或 `337`。
其中 3 个错误在同案件下一轮重复。所有错误回合的入库输入均为相同提示，
模型配置均为 `deepseek-flash` / `low`。本轮没有宿主机同条件或不同思考程度
对照，不能将算术错误归因于 Cube。探针脚本现支持
`VERIFY_EXPECTED_ANSWER=323`，可在未来同类负载中把正确率写入报告并在
不匹配时失败；上述原始报告产生时还没有这个断言。单案件真实 Cube 正向
复测的冷/暖 `correct=1`，故意使用错误预期 `999` 时两轮
`incorrect=1` 且脚本以非零状态退出，两个临时实例均已释放。报告分别见
`/tmp/erp-cube-acceptance/expected-answer-one-20260926/report.json` 与
`/tmp/erp-cube-acceptance/expected-answer-reject-20260926/report.json`。

同步运行资源采样命令：

```sh
RESOURCE_OUTPUT=/tmp/erp-cube-acceptance/model-turn-20260926-resources.json \
node --env-file=.env scripts/sample-agent-runtime-resources.mjs
```

采样按 500 ms 间隔共取得 172 个样本。峰值 PostgreSQL 连接 120，
`host_sandbox` 指标最多 51 条
（含原有实例），合计内存 416 MiB，内存限额失败 0；最低主机可用内存
42,913 MiB，memory PSI 为 0，swap-out 增量 0 页。该 Cubelet 仍未导出
`guest_workload` 指标，416 MiB 不是 guest 工作集，也无法证明每个 1 GiB
沙箱的内存余量。短提示无工具、无附件、无长上下文；这些数字不能代替
混合负载、长期稳态、跨节点或生产容量验收。

### 2026-09-26 受控环境读取复测

按用户要求，本地 `.runtime/cube.env` 的 `AGENT_TEMP_BASH_PROBE` 改为
`1`，重启 API 和八个 Worker；配置示例仍为 `0`。这只发布五条固定只读命令，
不会开放通用 shell、环境变量枚举、任意文件读取或写入。前端将此工具显示为
“读取运行环境”。

当前模板 `tpl-02d04202aa634f329df8d19f`（镜像
`127.0.0.1:5000/forge-agent-runtime:model-turn-20260926`，750m CPU /
1024 MiB）通过以下真实 Agent 回合读取 guest 环境：

```sh
VERIFY_CUBE_COUNT=1 \
VERIFY_CUBE_OUTPUT=/tmp/erp-cube-acceptance/bash-enabled-live-20260926 \
VERIFY_CUBE_MATCHED_PROMPT='请调用受限环境探针，依次读取 pwd、id、uname -a、/etc/os-release 和 /proc/1/cgroup。只汇总虚拟环境信息，不要执行其他命令。' \
node --env-file=.env --env-file=.runtime/cube.env scripts/verify-cube-resident-live.mjs
```

报告位于 `/tmp/erp-cube-acceptance/bash-enabled-live-20260926/report.json`。
案件 `55ed610e-e4ab-4f56-9e3b-89140d69cb74` 的冷回合消息 `2205`
完成且无错误，`agent_runs.tools` 中有五个 `bash:completed`；
PostgreSQL 会话 `session_4a3f93ac6d1342e788571d310f8a30a8` 的
`agent_session_entries` 保存了五个工具输入和原始结果：

| 命令 | 实际 guest 输出摘要 |
| --- | --- |
| `pwd` | `/home/forge/work` |
| `id` | `uid=10001(forge) gid=10001(forge) groups=10001(forge)` |
| `uname -a` | Linux；主机名 `tpl-02d0`；内核 `6.6.1199-0009-03_2.0.1`；`x86_64` |
| `cat /etc/os-release` | Ubuntu 24.04.5 LTS；`VERSION_ID=24.04`；`noble` |
| `cat /proc/1/cgroup` | `0::/default/tpl-02d04202aa634f329df8d19f_0/runtime` |

冷回合排队 16 ms、首正文 token 自提交起 2,066 ms、总耗时 7,562 ms，
工具耗时合计 2,348 ms。暖回合消息 `2206` 完成、总耗时 4,876 ms，
复用同一 runtime，但工具调用数为零：模型转述了前一轮结果，不能记为
第二次真实探测。两个回合均为 Cube backend，没有宿主机回退。单样本
不作为吞吐或 P95 结论。

测试实例 `68a1aba0c43b4361baf609fa09fde195` 已由脚本释放；核对时
Cube 无存活实例，API `/ready` 正常，九个用户级服务均为 active。
探针开关在 guest 初始化时读取；重启宿主 API/Worker 不会给已初始化的实例
热加载工具，后续新建实例才会按新配置加载。本地开关保持开启以满足本次
环境读取要求；默认配置和通用工具权限边界没有放宽。

Web 检查使用已保存的真实会话，没有再次调用模型：
`node --env-file=.env /tmp/erp-agent-environment-ui-20260926/verify.mjs`。
刷新前后均有五条“读取运行环境 / 已完成”记录，正文保留上述 guest 结果；
1440、390、320 px 的截图、报告和探针脚本位于
`/tmp/erp-agent-environment-ui-20260926/`。检查未发现横向页面溢出、
控件越界或浏览器 page error，`npm run typecheck` 和
`npm run build:web` 通过。本轮不代表通用 Bash、ERP 写入、
RAG 或生产部署验收。

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
