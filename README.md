# ERP Agent

<div align="center">

### Manufacturing changes, brought into focus.

**让每次制造变更看得清、查得到、有人把关。**

客户改了数量或交期，影响可能散落在订单、附件和生产上下文里。ERP Agent 把信息放进一个案件工作区，让 AI 帮团队找到依据、梳理影响、准备方案，再由业务人员决定下一步。

<p>
  <img src="docs/screenshots/final-desktop-chat.png" alt="ERP Agent 制造变更案件与人工审核界面" width="100%" />
</p>

<sub>产品界面示意 · 合成制造案例数据</sub>

<br><br>

[产品流程](#从变更到决策) · [产品界面](#案件资料与审核在同一个工作区) · [系统架构](#系统架构) · [本地运行](#本地运行) · [项目文档](#项目文档)

</div>

---

## 从变更到决策

一张订单变更单，往往牵动交期、数量、物料和现场安排。ERP Agent 将这些线索放回同一个业务上下文：团队可以查看案件资料，让 Agent 对照 ERP 与受限知识来源整理影响，再把建议变成可审阅的方案。

```text
客户变更  →  资料与订单  →  影响分析  →  人工审核  →  后续执行跟踪
```

## 案件资料与审核在同一个工作区

桌面端把案件、分析过程和变更提案放在一起；移动端可以查看证据来源和对应内容。团队可以持续补充上下文，也能回看提案依据，不必把重要判断留在一次性的聊天里。

<div align="center">
<table>
  <tr>
    <td align="center" valign="top" width="72%">
      <img src="docs/screenshots/final-desktop-chat.png" alt="桌面端制造变更案件与审核提案" width="100%" />
      <sub>案件分析与提案审核</sub>
    </td>
    <td align="center" valign="top" width="28%">
      <img src="docs/screenshots/final-mobile-evidence.png" alt="移动端案件证据与源文件定位" width="100%" />
      <sub>证据与原文定位</sub>
    </td>
  </tr>
</table>
</div>

## 为制造变更提供一条清晰路径

| 看清变化 | 找到依据 | 准备决策 |
| --- | --- | --- |
| 对照客户新要求与现有订单，整理数量、交期等变化。 | 将案件附件、ERP 只读记录和受限知识来源纳入分析上下文。 | 生成有版本和证据入口的提案，交由业务人员审核。 |

### 把决定权留在业务流程中

Agent 负责整理与建议，审批由人完成。后端负责权限、版本和业务状态检查；分析清单不是业务完成凭证，批准提案也不会绕过控制直接改写 ERP。

## 为真实业务边界而设计

- **案件有上下文：** 对话、资料、建议和审核围绕具体业务案件持续保存。
- **Agent 有边界：** 通过受控工具读取案件和 ERP 信息，不向业务模型开放通用 Bash 或任意文件编辑。
- **审批有依据：** 提案保留版本与证据入口，审批绑定明确的方案内容。
- **执行有校验：** 身份、授权、幂等和 ERP 状态复核由后端执行。

## 项目进展

当前是持续开发中的 Demo。案件工作区、对话恢复、图片问答、ERPNext 只读接入和 Cube 运行时探针已有本地验证；生产部署、完整业务验收、文档持久解析、知识检索和 ERP 受控写入仍在建设或待验收。

<details>
<summary>查看各模块的验证范围</summary>

| 领域 | 已验证 | 仍在建设或待验收 |
| --- | --- | --- |
| 案件工作区 | 邮箱注册登录、个人数据隔离、会话保存、PostgreSQL 重启恢复 | 生产部署与正式账户管理 |
| Agent 对话 | JCode Rust Loop、DeepSeek Responses、连续工具调用、上下文压缩与恢复 | 长时间混合负载与完整业务验收 |
| 文件资料 | RustFS 私有原件、图片问答；多种文档的受限读取路径通过本地测试 | 各文档格式的真实模型验收、MinerU 持久解析 |
| ERP 与知识 | ERPNext 真实列表/明细只读、MCP 工具和权限链路 | 审批后的受控写入、OpenViking 索引与 RAG |
| 运行环境 | Cube 常驻 Harness 的恢复及并发探针 | 长期性能测量、生产级部署与故障演练 |

详见[当前实施状态](docs/plantree/plans/server-demo/implementation-status.md)和[项目路线图](docs/plantree/plans/server-demo/roadmap.md)。

</details>

## 系统架构

<p align="center">
  <img src="docs/architecture/cube-jcode-server-architecture.png" alt="ERP Agent 服务端架构图" width="100%" />
</p>

<p align="center"><sub>沙箱内 JCode 运行方式的候选架构；部署位置仍以实测和架构记录为准。</sub></p>

## 技术栈

- **Web：** React、TypeScript
- **业务 API 与运行桥：** Rust、Axum、JCode
- **持久化与文件：** PostgreSQL、RustFS
- **演示 ERP：** ERPNext，通过受控 MCP 工具访问
- **沙箱：** CubeSandbox
- **规划中的知识链路：** OpenViking、MinerU

## 人工审核，业务有界

Agent 可以帮忙读资料、查订单、比较变更并准备提案。它的权限由后端按案件限定；审批人确认具体方案后，系统仍需重新检查 ERP 状态。当前版本接通的是 ERPNext 只读链路，审批不会自动触发 ERP 写入。

## 项目文档

- [产品架构](docs/architecture/erp-agent-demo-architecture.md)
- [实施状态与验证记录](docs/plantree/plans/server-demo/implementation-status.md)
- [项目路线图](docs/plantree/plans/server-demo/roadmap.md)
- [运行时部署方式](docs/architecture/cube-runtime-placement.md)
- [JCode 上游与版本说明](external/README.md)

## 本地运行

需要 Rust 工具链、Node.js 22.13+ 和 Docker Compose。克隆后在项目根目录执行：

```sh
git submodule update --init --recursive
npm ci
npm run dev:init
npm run db:up
npm run db:migrate
npm run db:seed
npm run storage:up
npm run storage:init
npm run dev:api
```

另开一个终端启动 Web：

```sh
npm run dev:web
```

访问 `http://127.0.0.1:4173`。首次初始化会生成本地开发配置；连接模型需要另外配置专用 API Key。详细配置见 [Runtime Bridge 指南](services/runtime-bridge/README.md)。

运行测试：

```sh
npm run check
```

更多 API、Worker、文件解析和 Cube 配置请从[服务说明](services/api/README.md)及[项目文档](docs/plantree/README.md)进入。
