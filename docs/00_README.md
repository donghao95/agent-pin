# Agent Pin 文档索引

本文是 Agent Pin 文档入口，负责说明事实源、阅读路径和维护规则；它不替代各领域规格。

仓库入口和本地启动说明见 [../README.md](../README.md)。本文只负责 `docs` 目录下的规格导航。

## 1. 文档分层

当前文档分为三层：

| 层级 | 作用 | 文件 |
|---|---|---|
| 开发宪法 | 项目目标、边界约束、第一性原理、提交规范 | [../AGENTS.md](../AGENTS.md) |
| 入口与状态 | 事实源入口和实现进度 | `00_README.md`（本文） |
| 核心规格 | 产品、架构、API、CLI、UI、分期、Skill | `01` 至 `07` |

核心规格清单：

| 编号 | 文件 | 职责 |
|---|---|---|
| 01 | [01_product_spec.md](01_product_spec.md) | 产品定位、Pin JSON、Block 规则、窗口与托盘行为、错误码、关键原则 |
| 02 | [02_architecture.md](02_architecture.md) | 模块划分、数据流、窗口管理、存储设计、入口与状态更新职责 |
| 03 | [03_api.md](03_api.md) | HTTP API 契约、错误码、请求/响应格式 |
| 04 | [04_cli.md](04_cli.md) | Rust CLI 行为、命令列表、路径转换、endpoint 校验 |
| 05 | [05_ui_style.md](05_ui_style.md) | Pin 窗口与管理界面视觉风格、Windows WebView2 退化说明 |
| 06 | [06_phase_plan.md](06_phase_plan.md) | Phase 1/2 分期、MVP 验收标准、后续路线 |
| 07 | [07_skill_design.md](07_skill_design.md) | Skill 设计原则（给外部 Agent 用的 `skills/agent-pin/SKILL.md` 的设计说明） |

辅助资源（不在 docs/ 下，但属于文档体系）：

- [../skills/agent-pin/SKILL.md](../skills/agent-pin/SKILL.md)：给外部 Agent 使用的通用 skill
- [../examples/pins/](../examples/pins/)：Pin JSON 示例
- [../prompts/](../prompts/)：给实现 Agent 的历史提示词（implement-mvp.md / review-mvp.md）

## 2. 冲突处理规则

遇到文档冲突时，按以下顺序判断：

1. 开发宪法和边界约束，以 [../AGENTS.md](../AGENTS.md) 为准。
2. 最新分期口径和 MVP 验收标准，以 `06_phase_plan.md` 为准。
3. 产品规格（Pin JSON、Block 规则、窗口/托盘行为），以 `01_product_spec.md` 为准。
4. 架构、模块、数据流、存储设计，以 `02_architecture.md` 为准。
5. HTTP API 契约和错误码，以 `03_api.md` 为准。
6. CLI 行为契约，以 `04_cli.md` 为准。
7. UI 视觉风格和退化说明，以 `05_ui_style.md` 为准。
8. Skill 设计原则，以 `07_skill_design.md` 为准。

如果实现与文档不一致：

```text
已明确决定的实现变更 -> 同步更新对应文档。
发现实现偏离规格且没有决策记录 -> 先定位根因，再决定改实现还是改文档。
外部说明与当前规格冲突 -> 当前规格生效。
```

## 3. MVP 边界

Agent Pin 是本地桌面工具，让 Agent 通过 CLI / HTTP 把重要内容推送成独立桌面 Pin 窗口。

核心不变量：

> Agent 可以把重要内容稳定、轻量、低打扰地贴到桌面上。

MVP 已实现（Phase 1 + Phase 2）：

- Tauri 2 桌面应用 + 系统托盘
- 本地 HTTP API（`127.0.0.1:4317`）
- Rust `agent-pin` CLI
- Pin 支持 markdown / image / status blocks，可多 block 混排
- Pin 窗口可拖动、缩放、置顶、关闭
- 文件系统持久化 + 管理界面 + 托盘快恢
- 应用重启后历史仍在

MVP 明确不做（见 [../AGENTS.md](../AGENTS.md)）：

- 不做 choice
- 不做点击事件回流
- 不做 Agent 自动继续执行
- 不做完整 HTML Artifact
- 不做 MCP Server
- 不做云同步、账号、远程访问

## 4. 阅读路径

- **首次了解项目**：[../AGENTS.md](../AGENTS.md) → `01_product_spec.md` → `06_phase_plan.md`
- **调用 HTTP API**：`03_api.md`
- **使用 CLI**：`04_cli.md` → [../skills/agent-pin/SKILL.md](../skills/agent-pin/SKILL.md)
- **理解架构与数据流**：`02_architecture.md`
- **调整 UI 风格**：`05_ui_style.md`
- **设计 Skill**：`07_skill_design.md` → [../skills/agent-pin/SKILL.md](../skills/agent-pin/SKILL.md)

## 5. 文档维护规则

- 修改 Pin JSON 结构时，必须同步更新：`01_product_spec.md` §7、`03_api.md`、`04_cli.md`、`skills/agent-pin/SKILL.md`、`examples/pins/`。
- 修改 HTTP API 时，必须同步更新：`03_api.md`、`04_cli.md`（如影响 CLI）、`skills/agent-pin/SKILL.md`（如影响 Agent 调用）。
- 修改 CLI 行为时，必须同步更新：`04_cli.md`、`skills/agent-pin/SKILL.md`。
- 未落地、正在讨论或包含未来路线判断的功能方案，不要写成已实现事实。未来功能写入 `06_phase_plan.md` 后续路线或单独的 product-notes。
- 功能实现完成并验收通过后，再把稳定事实合并进对应核心规格。
