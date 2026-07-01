# Agent Pin

[![CI](https://github.com/donghao95/agent-pin/actions/workflows/ci.yml/badge.svg)](https://github.com/donghao95/agent-pin/actions/workflows/ci.yml)
[![Release](https://github.com/donghao95/agent-pin/actions/workflows/release.yml/badge.svg)](https://github.com/donghao95/agent-pin/releases)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
[![GitHub release](https://img.shields.io/github/v/release/donghao95/agent-pin)](https://github.com/donghao95/agent-pin/releases)

Agent Pin 是一个本地桌面工具，让 Agent 可以把重要内容像 PixPin 一样贴到桌面上。

第一版目标很克制：Agent 通过 `agent-pin` CLI 或本地 HTTP API，把 Markdown、图片和状态信息推送成一个独立桌面 Pin 窗口。用户只负责查看、移动、缩放、关闭。MVP 不做 choice、不做点击反馈、不做 Artifact。

## MVP 一句话

> Agent 通过 CLI / HTTP 把 Markdown、图片、状态信息推送成独立桌面 Pin 窗口。

## 项目状态

当前处于 MVP 阶段，Phase 1 + Phase 2 已实现：

- ✅ Tauri 桌面应用 + 系统托盘
- ✅ 本地 HTTP API：`127.0.0.1:4317`
- ✅ `agent-pin` CLI（Rust）
- ✅ 一个 Pin 一个独立窗口，支持 `markdown` / `image` / `status` blocks
- ✅ 一个 Pin 可以包含多个 block
- ✅ Pin 窗口支持拖动、缩放、置顶、关闭
- ✅ 最近 Pin 历史 + 关闭后可恢复
- ✅ 独立管理界面（搜索 / 删除）
- ✅ 应用重启后历史保留
- ✅ Agent Skill 文档
- ✅ 更新检查（启动时静默检查 + 托盘菜单手动检查）

明确不做：choice、点击事件回流、Artifact、MCP Server、云同步、远程访问、截图/OCR、复杂主题系统。

## 安装

### 方式 1：下载 Release（推荐普通用户）

从 [Releases](https://github.com/donghao95/agent-pin/releases) 下载：

- `Agent-Pin_x.x.x_x64-setup.exe` — Windows 安装包，双击安装
- `agent-pin-vx.x.x-windows-x64.zip` — CLI 二进制，解压后将 `agent-pin.exe` 放到 PATH 中

安装后启动 Agent Pin，托盘出现图标即表示 HTTP 服务已在 `127.0.0.1:4317` 监听。

### 方式 2：从源码构建

前置：Node.js ≥ 20、pnpm 10.x、Rust stable、Windows 11。

```bash
git clone https://github.com/donghao95/agent-pin.git
cd agent-pin
pnpm install
.\scripts\dev.ps1 # Windows 本地调试脚本
pnpm dev          # 开发模式（等价的底层命令）
pnpm build        # 生产构建（输出 NSIS 安装包）
```

CLI 单独构建：

```bash
cd packages/cli
cargo build --release
# 产物：packages/cli/target/release/agent-pin.exe
```

## 使用

### 通过 CLI（推荐 Agent 使用）

```bash
agent-pin health
agent-pin markdown --title "PR 审查结果" --text "## 结论\n通过"
agent-pin image --title "效果图" --path ./render.png
agent-pin status --title "构建状态" --level success --text "构建通过"
agent-pin list
agent-pin show <pinId>
agent-pin hide-all
```

完整 CLI 说明见 [docs/04_cli.md](docs/04_cli.md)。
Agent 使用规则与示例见 [skills/agent-pin/SKILL.md](skills/agent-pin/SKILL.md)。

### 通过 HTTP API（底层入口）

```bash
curl -X POST http://127.0.0.1:4317/api/pins \
  -H "Content-Type: application/json" \
  -d '{"version":1,"title":"测试 Pin","blocks":[{"type":"markdown","content":"## Hello Agent Pin\n这是第一个 Pin。"}]}'
```

完整 API 说明见 [docs/03_api.md](docs/03_api.md)。

## 项目结构

```text
agent-pin/
  apps/desktop/         # Tauri 桌面应用
  packages/cli/         # agent-pin CLI（Rust）
  packages/shared/      # 共享类型和校验逻辑（PinDocument 契约）
  docs/                 # 产品与工程规格
  skills/agent-pin/     # 给外部 Agent 使用的通用 skill
  prompts/              # 给 Codex / Claude / Trae 的实现提示词
  examples/pins/        # Pin JSON 示例
  scripts/              # 维护脚本（版本同步等）
  .github/              # CI / Issue / PR 模板
```

## 关键文档

- [文档索引](docs/00_README.md)
- [产品规格](docs/01_product_spec.md)
- [架构](docs/02_architecture.md)
- [HTTP API](docs/03_api.md)
- [CLI](docs/04_cli.md)
- [UI 风格](docs/05_ui_style.md)
- [分期与验收](docs/06_phase_plan.md)
- [Skill 设计](docs/07_skill_design.md)
- [通用 skill（给外部 Agent）](skills/agent-pin/SKILL.md)

## 开发

参与开发请先读 [AGENTS.md](AGENTS.md)（项目宪法）与 [CONTRIBUTING.md](CONTRIBUTING.md)。请遵守 [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)。

提交前本地检查：

```bash
pnpm sync-version:check
pnpm lint
pnpm typecheck
cargo check --manifest-path packages/shared/Cargo.toml --locked
cargo check --manifest-path packages/cli/Cargo.toml --locked
cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml --locked
```

发版流程：修改根 `package.json` 的 `version` → 运行 `pnpm sync-version` → 提交 → 打 tag `v0.x.x` → 推送 tag 触发 Release 工作流。

## 安全

HTTP 服务只监听 `127.0.0.1`，不开放局域网。无身份验证、无远程访问能力——这是 MVP 的明确边界。漏洞报告流程见 [SECURITY.md](SECURITY.md)。

## License

[Apache License 2.0](LICENSE)
