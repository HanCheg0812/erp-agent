# Cube Web UI 本地验证

日期：2026-09-26。范围：现有 Cube v0.7.2 控制台的访问、代理、服务托管和监控页面验证。

## 当前状态

本机已有官方安装目录中的 Web UI，复用现有部署。管理员登录和 11 个主栏目、设置的 5 个标签已通过真实浏览器验证，模拟数据开关保持关闭。本次没有重装 Cube、重置管理员密码、修改业务 API/Worker 或操作沙箱生命周期。

## 入口与服务

- 本机入口：`http://127.0.0.1:12088/`。
- 当前局域网入口：`http://192.168.100.122:12088/`。本机通过该地址返回 200；尚未从第二台设备验证，主机 IP 变化时应重新确认。
- systemd：`cube-sandbox-webui.service`，`active/running`，`Restart=on-failure`。
- 服务归属：已启用的 `cube-sandbox-control.target` 通过 `Wants` 启动 Web UI。Web UI 单元自身显示 `disabled`，不代表没有开机启动依赖。
- 容器：`cube-webui`，健康状态为 `healthy`，`unless-stopped` 重启策略，主机端口 `12088` 映射到容器 `80`。
- 静态产物：`/usr/local/services/cubetoolbox/webui/dist`，以只读卷挂载。
- Nginx 配置：`/usr/local/services/cubetoolbox/webui/nginx.generated.conf`，以只读卷挂载；`nginx -t` 通过。

实际代理配置中，`/opsapi/` 转到 CubeOps `:3010/api/`，`/cubeapi/v1/` 转到其 `/api/v1/sdk/`；`/health` 转到 CubeAPI `:3001`。应以当前安装配置和浏览器请求为准，不能只凭官方说明中概括的端口判断。

## 已验证

| 检查 | 结果 |
| --- | --- |
| 本机及当前局域网地址 HTTP GET `/` | 均为 200 |
| Web UI GET `/health` | 200，`status=ok`，当时 `sandboxes=0` |
| 未登录 GET `/opsapi/v1/auth/session` | 401 |
| 真实浏览器 | 正常显示中文登录页，未见脚本错误或白屏 |
| 管理员登录 | 成功进入控制台；只读校验现有 bcrypt 哈希后，确认官方默认凭据仍有效，未重置密码 |
| 登录失败复核 | 先前输入未同步到表单状态；原生输入过程中也曾出现用户名重复。纠正输入后成功，先前错误不证明密码已变更 |
| 概览 / 沙箱 | 0 个运行中沙箱，30 个模板；当前没有沙箱可验收资源曲线或生命周期按钮 |
| 节点 / 版本 / 网络 | 192.168.100.122 持续上报 v0.7.2；192.168.3.5 保留约 7 天前的旧记录，网络与版本页判为异常 |
| 可观测性 | 健康节点 1/2，模板正常 20 / 失败 10；网关连通性检查先后显示 17 ms、12 ms，均为连接正常 |
| 组件仓库 | 4 类组件库存均未导入；页面可用不代表组件分发已验收 |
| 模板市场 / 数字助手 | 官方模板目录可见；数字助手显示预览版、个人及团队助手均为 0，未配置或创建 OpenClaw/Hermes 助手 |
| 设置 | 外观、集群连接、账号、快捷键、关于均可打开；集群连接测试显示连接正常、5 ms；未修改配置 |
| Docker / systemd | 容器健康，服务运行，控制服务组和 Docker 均已启用 |

```sh
curl --fail http://127.0.0.1:12088/health
curl --output /dev/null --write-out '%{http_code}\n' http://192.168.100.122:12088/
systemctl show cube-sandbox-webui.service cube-sandbox-control.target -p ActiveState -p SubState -p UnitFileState -p Restart
docker inspect cube-webui --format '{{json .HostConfig.RestartPolicy}}'
docker exec cube-webui /usr/local/openresty/nginx/sbin/nginx -t
```

## 截图证据

截图主要为实际浏览器视口 1280x800，首张概览为 1270x823。全页截图在固定布局中出现拼接重复，已改为实际视口截图；可观测性另留滚动到连通性和失败模板区域的截图。截图保存在本机 `/tmp/cube-console-screenshots/`，临时目录不作为长期存储，未提交图片或凭据。

| 页面 | 文件 |
| --- | --- |
| 概览 | `01-overview.png` |
| 沙箱 | `02-sandboxes.png` |
| 模板 | `03-templates.png` |
| 节点 | `04-nodes.png` |
| 版本 | `05-versions.png` |
| 组件仓库 | `06-warehouse.png` |
| 网络 | `07-network.png` |
| 可观测性 | `08-observability.png`、`08b-observability-errors.png`、`08c-observability-connectivity.png` |
| 模板市场 | `09-store.png` |
| 数字助手 | `10-agenthub.png` |
| 设置 | `11-settings-appearance.png`、`12-settings-cluster.png`、`13-settings-account.png`、`14-settings-shortcuts.png`、`15-settings-about.png` |

## 发现与后续验证

- 概览把旧节点纳入总容量，显示 96 核、155 GiB 和 310 槽位；当前健康节点实际报告 48 核、77.8 GiB 和 155 槽位。节点列表虽仍显示旧记录的 Ready=True，但心跳约 7 天前，不能据此认定在线。
- 失败模板是历史构建记录，不等于当前运行失败。页面保留了健康探针连接拒绝、VmShutdown 和未限定仓库的镜像拉取鉴权错误；部分记录没有错误详情，本轮未重试或删除。
- 控制台的 API 请求监控实际为手动连通性检查。上述毫秒值不是模型延迟、沙箱冷启动指标或负载测试 P95。Cube WebUI 本身未验证为 Prometheus/Alertmanager 告警管理入口；当前 Cube 部署已由旁侧的 Prometheus、Alertmanager 和 Grafana 持久化采集 Cube 端点、Cubelet 资源指标及 systemd 状态，具体证据见[监控部署记录](monitoring-deployment.md)。

## 待验证与边界

- 有运行中沙箱时，继续核对详情、资源上报、事件/日志与 Forge 持久化记录；本轮没有制造新的负载。
- 系统安装的 `.one-click.env` 当前用户无权读取；凭据不写入项目文档、截图或 Git。
- 管理员仍使用官方默认凭据；本轮按现有账号登录，没有执行密码变更。管理入口的凭据强化尚未验收。
- 开机恢复依据当前 systemd 依赖和 Docker 策略；本轮没有重启主机进行验收。
- 未修改监听范围、防火墙、TLS 或管理 API 鉴权；本地访问成功不代表已完成公网部署。
- 官方 Web UI 提供运行状态视图；长期指标存储与告警由 Cube 部署配套的 Prometheus/Alertmanager/Grafana 承担，当前已完成本机回环验证。Cube guest 工作负载指标、跨节点容量、外部通知以及 Forge 队列/模型/工具业务负载仍需独立验收。

## 模板选择复核

2026-09-26 后续核对：在已登录的原生 `/sandboxes/new` 页面，模拟数据关闭，READY 模板可以直接选择。实际选中 `tpl-5f913dc0f4b3471c92d2fb18` 浏览器模板后，模板高亮且“创建沙箱”按钮可用，截图已在对话中展示；本次没有点击创建。与官方 v0.7.2 的 `SandboxNew.tsx`、`SandboxDetail.tsx` 和 `api/client.ts` 对照，新建请求包含 `templateID`，已有通用实例详情提供暂停、恢复、销毁，没有原地替换模板接口。

Cube Web 的模板选择不会自动建立 Forge 的租户/会话身份、Bridge 配置或工具映射；这部分现由 Rust API 的受控切换接口接线，仍不把 Cube Web 的通用实例直接当作业务会话。原生数字助手创建流程的模板选择目前属于 OpenClaw 路径。详细边界、切换行为和实际 JCode 浏览器工具验收见[模板接入计划](runtime-management.md#native-cube-web-template-selection)。

官方资料：[Web UI](https://cubesandbox.com/zh/guide/webui)、[v0.7.2 对应文档](https://github.com/TencentCloud/CubeSandbox/blob/v0.7.2/docs/zh/guide/webui.md)。关联：[本地 Cube 运维](../../../../infra/cube/README.md)、[常驻运行时](cube-resident-runtime.md)。
