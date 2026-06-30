# Contributing to Agent Pin

感谢你对 Agent Pin 的兴趣。本文档说明如何参与贡献。

## 项目状态

Agent Pin 当前处于 MVP 阶段（Phase 1 + Phase 2 已实现）。仓库目标：

> Agent 通过 CLI / HTTP 把 Markdown、图片、状态信息推送成独立桌面 Pin 窗口。

明确不做：choice、点击事件回流、Artifact、MCP Server、云同步、远程访问。完整边界见 [AGENTS.md](AGENTS.md)。

## 开发环境

| 工具 | 版本 |
|---|---|
| Node.js | ≥ 20 |
| pnpm | 10.4.0 |
| Rust | stable |
| 操作系统 | Windows 11（主验收平台） |

仓库结构、本地开发命令、验收命令见 [AGENTS.md](AGENTS.md) 的"项目结构"、"本地开发命令"、"测试与验收要求"。

## 提交前检查

每次提交前必须本地通过：

```bash
pnpm sync-version:check    # 版本号一致性
pnpm lint                  # ESLint
pnpm typecheck             # tsc --noEmit
cargo check --manifest-path packages/shared/Cargo.toml --locked
cargo check --manifest-path packages/cli/Cargo.toml --locked
cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml --locked
```

CI（[.github/workflows/ci.yml](.github/workflows/ci.yml)）会跑同样的检查，本地先通过能避免来回 round-trip。

## 对抗式审查

本项目要求：任何提交、PR、请求验收之前，必须进行对抗式审查（adversarial review）。

要求：

- 在支持子代理的环境中，使用独立子代理从反方角度审查：
  - 改动是否破坏 MVP 边界
  - 是否引入过度设计
  - 是否遗漏错误处理
  - 是否与文档契约冲突
- 审查发现的问题必须先处理或明确记录为后续问题，再提交。
- 不要把普通自我总结当作对抗式审查。

详见 [AGENTS.md](AGENTS.md) 的"对抗式审查规则"。

## Commit 规范

使用 Conventional Commits 风格：

```
<type>(<scope>): <subject>

[可选 body]
```

类型：

- `feat`：新功能
- `fix`：bug 修复
- `docs`：文档
- `refactor`：重构
- `chore`：杂项
- `test`：测试
- `ci`：CI 配置
- `build`：构建系统

scope 示例：`phase-1`、`phase-2a`、`phase-2b`、`phase-2c`、`cli`、`desktop`、`docs`。

## PR 规范

PR 描述必须包含（模板见 [.github/PULL_REQUEST_TEMPLATE.md](.github/PULL_REQUEST_TEMPLATE.md)）：

- 改了什么
- 没做什么
- 如何验证
- 是否更新文档

每个 PR 尽量只做一个阶段或一个主题，不要把 MVP 阶段不允许的功能（choice、事件回流、MCP、Artifact）混入。

## 文档同步

修改 Pin JSON 结构、CLI 契约、API 行为时，必须同步更新对应文档：

- 产品规格：`docs/01_product_spec.md`
- 架构：`docs/02_architecture.md`
- HTTP API：`docs/03_api.md`
- CLI：`docs/04_cli.md`
- UI 风格：`docs/05_ui_style.md`
- Skill：`skills/agent-pin/SKILL.md`
- 示例：`examples/pins/`

文档冲突处理见 [docs/00_README.md](docs/00_README.md)。

## 报告问题

- Bug / 功能请求：用 Issue 模板（`.github/ISSUE_TEMPLATE/`）
- 安全漏洞：见 [SECURITY.md](SECURITY.md)，不要在公开 Issue 中讨论
