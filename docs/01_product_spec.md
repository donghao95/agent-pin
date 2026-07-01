# Agent Pin 产品规格

版本：v0.1
阶段：MVP（Phase 1 + Phase 2 已实现）
状态：稳定

## 1. 背景

Agent 现在主要通过聊天框、终端或 IDE 对话窗口输出结果。很多结果很重要，但不适合一直留在聊天记录里，例如：

- PR 审查结论
- 长任务状态
- 风险提醒
- 图片结果
- 装修方案摘要
- 需要稍后查看的判断

Agent Pin 的目标是让 Agent 可以把这些内容直接贴到桌面上。

它不是聊天工具，也不是完整 Artifact 系统，而是一个轻量桌面 Pin 工具。

## 2. 产品定位

Agent Pin 是一个本地桌面应用。

它允许任意 Agent 通过 CLI / HTTP 把内容推送为一个独立桌面 Pin 窗口。

每个 Pin 类似 PixPin 的贴图窗口：

- 独立悬浮
- 可拖动
- 可缩放
- 可置顶
- 可关闭
- 可以显示 Markdown、图片、状态信息

MVP 核心闭环：

```text
Agent 生成内容
  ↓
调用 agent-pin CLI 或 HTTP API
  ↓
Agent Pin 桌面应用接收请求
  ↓
创建一个独立 Pin 窗口
  ↓
用户查看、移动、缩放、关闭
```

MVP 只做 Agent → App 的纯展示，不做 User → Agent 的事件回流。

## 3. MVP 必须做

- Tauri 2 桌面应用
- 系统托盘
- 本地 HTTP API，监听 `127.0.0.1:4317`
- `agent-pin` CLI（Rust 实现）
- 一个 Pin 对应一个独立桌面窗口
- Pin 支持 `markdown` / `image` / `status` blocks
- 一个 Pin 可以包含多个 block
- Pin 窗口支持拖动、缩放、置顶、关闭
- Markdown 支持标题、列表、引用、代码块、链接、Markdown 表格
- Image 支持本地图片路径
- Status 支持 `info` / `success` / `warning` / `error`
- 简单最近 Pin 历史，关闭后可恢复（混合方案：托盘快恢最近 5 个 + 管理界面完整历史）
- 基础错误处理，坏输入不能导致应用崩溃
- Agent 使用 skill 文档
- 更新检查（启动时静默检查 + 托盘菜单手动检查，调 GitHub API，24h 缓存）

## 4. MVP 明确不做

- 不做 choice
- 不做点击事件回流
- 不做 Agent 自动继续执行
- 不做完整 HTML Artifact
- 不做网页分享
- 不做结构化 table block（表格用 Markdown 表格表达）
- 不做云同步和账号系统
- 不做远程访问
- 不做截图、OCR、录屏
- 不做图片编辑
- 不做 MCP Server
- 不做复杂标签、分类、搜索
- 不做实时 status 更新
- 不做 WebSocket 实时通道

## 5. 技术路线

- 桌面框架：Tauri 2
- 前端：React + TypeScript
- 后端：Rust / Tauri backend
- CLI：Rust 实现（`packages/cli/`），底层调用本地 HTTP API
- API：本地 HTTP 服务，默认 `http://127.0.0.1:4317`

HTTP 只监听 `127.0.0.1`，不开放局域网。

## 6. 核心概念

### Pin

Pin 是一个独立桌面窗口，是内容容器。

### Block

Block 是 Pin 中的内容块。MVP 支持：

- `markdown`
- `image`
- `status`

一个 Pin 可以包含多个 block，例如：Markdown + Image + Markdown。

### Source

Source 表示 Pin 的来源 Agent、workspace 或任务。MVP 中 source 可选，不影响渲染。

## 7. Pin JSON 结构

基础结构：

```json
{
  "version": 1,
  "title": "PR 审查结果",
  "blocks": [
    {
      "type": "markdown",
      "content": "## 结论\n发现 2 个问题，建议先修第一个。"
    }
  ],
  "window": {
    "width": 420,
    "height": "auto",
    "alwaysOnTop": true
  },
  "source": {
    "agent": "codex",
    "workspace": "TryCue",
    "task": "PR Review"
  },
  "createdAt": "2026-06-30T12:00:00+08:00"
}
```

TypeScript 类型：

```ts
export type PinDocument = {
  version: 1
  title: string
  blocks: PinBlock[]
  window?: PinWindowConfig
  source?: PinSource
  createdAt?: string
}

export type PinBlock = MarkdownBlock | ImageBlock | StatusBlock

export type MarkdownBlock = {
  type: 'markdown'
  content: string
}

export type ImageBlock = {
  type: 'image'
  path: string
  caption?: string
}

export type StatusBlock = {
  type: 'status'
  level?: 'info' | 'success' | 'warning' | 'error'
  text: string
}

export type PinWindowConfig = {
  width?: number
  height?: number | 'auto'
  x?: number
  y?: number
  alwaysOnTop?: boolean
}

export type PinSource = {
  agent?: string
  workspace?: string
  task?: string
  conversationId?: string
}
```

PinDocument 是 API、CLI、窗口渲染之间的核心契约。修改 Pin JSON 结构时，必须同步更新：`03_api.md`、`04_cli.md`、`skills/agent-pin/SKILL.md`、`examples/pins/`。

### 字段长度上限（防御性校验）

`packages/shared` 的 `validate()` 会对以下字段做长度上限校验，超限返回 `INVALID_PIN_DOCUMENT`：

| 字段 | 上限 | 说明 |
| --- | --- | --- |
| `title` | 1024 字符 | 防止标题过长导致窗口标题栏溢出 |
| `markdown.content` | 256 KB | 单个 markdown block 内容上限 |
| `blocks` 数量 | 50 | 单个 Pin 的 block 数量上限 |
| `window.width` / `window.height`（数字值） | 1..=100_000 | 防止异常大值导致窗口创建失败 |

HTTP 请求体总大小上限为 1 MB（`03_api.md` §4）。

## 8. Block 规则

### Markdown Block

结构：

```json
{
  "type": "markdown",
  "content": "## 结论\n这是 Markdown 内容。"
}
```

支持：标题、段落、列表、引用、代码块、行内代码、链接、Markdown 表格。

暂不支持：Mermaid、LaTeX、HTML 注入、iframe、复杂图表。

### Image Block

结构：

```json
{
  "type": "image",
  "path": "C:/Users/hao/Desktop/example.png",
  "caption": "错误截图"
}
```

支持：本地 PNG、JPG/JPEG、WebP、GIF。

`path` 必须是绝对路径。相对路径解析在 CLI 实现（CLI 把相对路径转绝对再 POST，见 `04_cli.md` §5/§7）。直接通过 HTTP 测试时需传绝对路径。

图片路径不存在时，不应导致应用崩溃，应在 Pin 内显示错误块。

### Status Block

结构：

```json
{
  "type": "status",
  "level": "warning",
  "text": "PR 审查完成：发现 1 个高风险问题。"
}
```

`level` 可选值：`info`、`success`、`warning`、`error`。默认 `info`。

MVP 中 status 是静态展示，不做实时更新。

## 9. 窗口行为

每个 Pin 是一个独立窗口。

默认行为：

- 默认置顶
- 可拖动
- 可缩放
- 可关闭
- 内容可滚动
- 图片自动适配宽度
- 关闭不删除数据（关闭 = hidden，可恢复）
- 多个 Pin 级联排列，避免完全重叠

默认尺寸：

```json
{
  "width": 420,
  "height": "auto",
  "alwaysOnTop": true
}
```

约束：

- 最小宽度：280
- 默认宽度：420
- 最大默认高度：屏幕高度的 70%

窗口类型：

- **Pin 窗口**（label 是 pinId）：`decorations(false)` + `shadow(true)` + 自定义轻标题栏 + `alwaysOnTop=true` + `skipTaskbar=true`
- **管理界面窗口**（label 固定为 `manager`）：`decorations(true)` 系统装饰 + `resizable(true)` + 880×620 + min 640×400

窗口行为：

- Pin 窗口关闭 = destroy + state=hidden（可恢复）
- 管理界面窗口关闭 = hide（缩回托盘，窗口实例保留）；托盘"打开管理界面"重新 show
- 应用启动自动打开管理界面窗口
- 只有托盘"退出 Agent Pin"才退出 app

窗口 label（即 pinId）格式：`pin_<timestamp_ms>_<6位随机数字>`，例如 `pin_1782801843675_717272`。

详细窗口管理设计见 `02_architecture.md` §4。

## 10. 托盘行为

应用启动后常驻系统托盘。

托盘菜单（Phase 2-B 实现口径）：

- 最近 5 个 hidden Pin（点击快速重新打开，即"快恢"）
- 打开管理界面（完整历史 + 搜索 + 删除）
- 隐藏全部 Pin
- 检查更新（调 GitHub API 查最新 release，有新版打开浏览器到 Release 页）
- 退出 Agent Pin

托盘只做应用存活 + 快恢入口 + 退出。Pin 完整生命周期管理（完整历史、搜索、删除）走独立的管理界面窗口（`?manager=1`）。

MVP 可以没有完整主窗口，只保留托盘、Pin 窗口和管理界面窗口。

### 更新检查（轻量方案）

- 应用启动时静默检查一次（异步、不阻塞、失败忽略）
- 托盘"检查更新"菜单项：点击后强制检查，有新版弹 dialog 提示，确认后打开浏览器到 Release 页
- 数据源：GitHub `releases/latest` API
- 缓存：`~/.agent-pin/update-cache.json`，24 小时内不重复请求（避免触发 GitHub API 限流）
- MVP 不做自动下载安装、签名验证（留给正式产品化阶段）

## 11. 错误处理

任何坏输入都不能导致应用崩溃。

错误码定义见 `03_api.md` §2。常见错误：

- `INVALID_JSON`：JSON 解析失败或未知 block type（serde 反序列化阶段拒绝）
- `INVALID_PIN_DOCUMENT`：version/title/blocks 非空/path 绝对路径/level 枚举校验失败
- `UNSUPPORTED_BLOCK_TYPE`：保留，当前未知 type 走 `INVALID_JSON`
- `IMAGE_NOT_FOUND`：保留，HTTP 层不校验文件存在性，前端 `<img>` onerror 兜底
- `IMAGE_UNSUPPORTED`：图片扩展名非 PNG/JPG/JPEG/WebP/GIF
- `PIN_NOT_FOUND`：show/hide/delete 路由中 pinId 不存在
- `WINDOW_CREATE_FAILED`：窗口创建失败
- `INTERNAL_ERROR`：内部错误

## 12. 关键原则

- 第一版只做纯展示
- 一个 Pin 是一个独立桌面窗口
- Pin 是容器，block 是内容
- Agent 优先通过 CLI 使用
- CLI 底层调用本地 HTTP
- 不要让 Agent 乱推 Pin
- 不要把聊天记录全文 pin 到桌面
- 每个 Pin 应该有明确用途
