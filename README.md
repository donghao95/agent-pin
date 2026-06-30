# Agent Pin

Agent Pin 是一个本地桌面工具，让 Agent 可以把重要内容像 PixPin 一样贴到桌面上。

第一版目标很克制：Agent 通过 `agent-pin` CLI 或本地 HTTP API，把 Markdown、图片和状态信息推送成一个独立桌面 Pin 窗口。用户只负责查看、移动、缩放、关闭。MVP 不做 choice、不做点击反馈、不做 Artifact。

## MVP 一句话

> Agent 通过 CLI / HTTP 把 Markdown、图片、状态信息推送成独立桌面 Pin 窗口。

## 第一版做什么

- Tauri 桌面应用
- 系统托盘
- 本地 HTTP API：`127.0.0.1:4317`
- `agent-pin` CLI
- 一个 Pin 一个独立窗口
- 支持 `markdown` / `image` / `status` blocks
- 一个 Pin 可以包含多个 block
- Pin 窗口支持拖动、缩放、置顶、关闭
- 最近 Pin 简单历史
- Agent 使用 skill 文档

## 第一版不做什么

- 不做 choice
- 不做用户点击事件回流
- 不做 Artifact 网页
- 不做结构化 table block
- 不做云同步和账号系统
- 不做远程访问
- 不做截图、OCR、录屏
- 不做 MCP Server

## 推荐目录

```text
agent-pin/
  docs/                 # 产品与工程规格
  skills/agent-pin/     # 给外部 Agent 使用的通用 skill
  prompts/              # 给 Codex / Claude / Trae 的实现提示词
  examples/pins/        # Pin JSON 示例
  apps/desktop/         # 后续 Tauri 桌面应用
  packages/cli/         # 后续 agent-pin CLI
  packages/shared/      # 后续共享类型和校验逻辑
```

## 关键文档

- [MVP 规格文档](docs/mvp-spec.md)
- [API 设计](docs/api.md)
- [CLI 设计](docs/cli.md)
- [Skill 设计](docs/skill-design.md)
- [实现 MVP 的提示词](prompts/implement-mvp.md)

## 最小目标

后续第一轮实现只需要跑通这个闭环：

```text
Agent / curl / CLI
  ↓
POST http://127.0.0.1:4317/api/pins
  ↓
Tauri App
  ↓
桌面出现一个独立 Markdown Pin 窗口
```
