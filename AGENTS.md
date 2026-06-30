# AGENTS.md

本文件给参与开发 Agent Pin 的 Codex / Claude / Trae / CodeBuddy 等开发 Agent 使用。

## 项目目标

Agent Pin 是一个本地桌面工具，让 Agent 可以通过 CLI / HTTP 把重要内容推送成独立桌面 Pin 窗口。

第一版只验证一个核心体验：

> Agent 生成的重要信息，能直接出现在桌面上，而不是淹没在聊天框里。

## MVP 范围

必须做：

- Tauri 2 桌面应用
- 系统托盘
- 本地 HTTP API，监听 `127.0.0.1:4317`
- `agent-pin` CLI
- 一个 Pin 一个独立窗口
- 支持 `markdown` / `image` / `status` blocks
- 一个 Pin 可以包含多个 block
- Pin 窗口支持拖动、缩放、置顶、关闭
- 最近 Pin 简单历史，关闭后可从托盘重新打开
- 基础错误处理，坏输入不能导致应用崩溃

明确不做：

- 不做 choice
- 不做点击事件回流
- 不做 Agent 自动继续执行
- 不做 Artifact 网页
- 不做 table block，表格用 Markdown 表格表达
- 不做云同步、账号、远程访问
- 不做截图、OCR、录屏、图片编辑
- 不做 MCP Server

## 技术路线

推荐：

- 桌面：Tauri 2
- 前端：React + TypeScript
- 后端：Rust / Tauri backend
- CLI：MVP 可用 Node.js CLI，后续可迁移 Rust CLI
- 本地 API：`http://127.0.0.1:4317`

## 开发顺序

严格按阶段做，不要一次性扩展：

1. Tauri 应用骨架 + 托盘
2. 本地 HTTP 服务：`GET /api/health`、`POST /api/pins`
3. 收到 Markdown Pin 后创建独立窗口
4. 支持 image/status blocks
5. 支持多 block 混排
6. 实现 `agent-pin` CLI
7. 最近 Pin 历史和托盘恢复
8. Skill 文档

## 目录约定

```text
apps/desktop/       # Tauri 桌面应用
packages/cli/       # agent-pin CLI
packages/shared/    # Pin JSON 类型、schema、校验逻辑
docs/               # 规格文档
skills/agent-pin/   # 给外部 Agent 使用的通用 skill
examples/pins/      # Pin JSON 示例
prompts/            # 给实现 Agent 的提示词
```

## Pin 数据模型原则

- Pin 是窗口。
- Block 是内容。
- 不要设计成 `MarkdownPin` / `ImagePin` 两套对象。
- 一个 Pin 可以混合 Markdown、图片和状态块。

核心结构：

```ts
export type PinDocument = {
  version: 1
  title: string
  blocks: PinBlock[]
  window?: PinWindowConfig
  source?: PinSource
  createdAt?: string
}
```

## 实现注意事项

- HTTP 只监听 `127.0.0.1`，不要开放局域网。
- `POST /api/pins` 必须校验请求体。
- 非法 JSON、空 blocks、图片不存在等情况不能让应用崩溃。
- 图片路径不存在时，在 Pin 内显示错误 block，而不是拒绝整个 Pin。
- Markdown 表格和代码块需要基本可读，窄窗口下可以横向滚动。
- 多个 Pin 创建时要级联排列，避免完全重叠。
- 关闭窗口不等于删除 Pin。

## PR / Commit 约束

- 每个 PR 尽量只做一个阶段。
- 不要在 MVP 阶段引入 choice、事件回流、MCP、Artifact。
- 不要提前做复杂美化、主题系统或云同步。
- 优先保证最小闭环可运行。

## 验收命令

第一阶段最重要的验收是：

```bash
curl -X POST http://127.0.0.1:4317/api/pins \
  -H "Content-Type: application/json" \
  -d '{"version":1,"title":"测试 Pin","blocks":[{"type":"markdown","content":"## Hello Agent Pin\n这是第一个 Pin。"}]}'
```

桌面应出现一个独立 Markdown Pin 窗口。
