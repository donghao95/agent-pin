# Agent Pin MVP 规格文档

版本：v0.1  
阶段：MVP  
状态：草案

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
- `agent-pin` CLI
- 一个 Pin 对应一个独立桌面窗口
- Pin 支持 `markdown` / `image` / `status` blocks
- 一个 Pin 可以包含多个 block
- Pin 窗口支持拖动、缩放、置顶、关闭
- Markdown 支持标题、列表、引用、代码块、链接、Markdown 表格
- Image 支持本地图片路径
- Status 支持 `info` / `success` / `warning` / `error`
- 简单最近 Pin 历史，关闭后可从托盘重新打开
- 基础错误处理，坏输入不能导致应用崩溃
- Agent 使用 skill 文档

## 4. MVP 明确不做

- 不做 choice
- 不做点击事件回流
- 不做 Agent 自动继续执行
- 不做完整 HTML Artifact
- 不做网页分享
- 不做结构化 table block
- 不做云同步和账号系统
- 不做远程访问
- 不做截图、OCR、录屏
- 不做图片编辑
- 不做 MCP Server
- 不做复杂标签、分类、搜索
- 不做实时 status 更新

## 5. 技术路线

- 桌面框架：Tauri 2
- 前端：React + TypeScript
- 后端：Rust / Tauri backend
- CLI：MVP 可用 Node.js CLI，后续可迁移为 Rust CLI
- API：本地 HTTP 服务，默认 `http://127.0.0.1:4317`

## 6. 架构

```text
Agent / Skill
  ↓
agent-pin CLI
  ↓
HTTP API: http://127.0.0.1:4317
  ↓
Tauri Rust Backend
  ↓
创建 Pin Window
  ↓
Web Frontend 渲染 Pin
```

文件夹投递可作为调试和兜底能力，但不是第一主路径。

## 7. 核心概念

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

## 8. Pin JSON

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

## 9. Block 规则

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

支持：本地 PNG、JPG/JPEG、WebP；GIF 可选。

`path` 必须是绝对路径。相对路径解析在 Phase 2-C CLI 实现（CLI 把相对路径转绝对再 POST）。直接通过 HTTP 测试时需传绝对路径。

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

## 10. HTTP API

默认地址：

```text
http://127.0.0.1:4317
```

只监听本地回环地址，不开放局域网。

### GET /api/health

响应：

```json
{
  "ok": true,
  "app": "Agent Pin",
  "version": "0.1.0"
}
```

### POST /api/pins

创建一个 Pin。

成功响应：

```json
{
  "ok": true,
  "pinId": "pin_20260630_121530_pr_review"
}
```

失败响应：

```json
{
  "ok": false,
  "error": {
    "code": "INVALID_PIN_DOCUMENT",
    "message": "blocks must contain at least one block"
  }
}
```

### GET /api/pins

列出最近 Pin。MVP 可简化。

### POST /api/pins/{pinId}/show

重新显示一个已隐藏 Pin。

### POST /api/pins/{pinId}/hide

隐藏一个 Pin。

### POST /api/pins/hide-all

隐藏全部 Pin。

## 11. CLI

CLI 名称：

```text
agent-pin
```

命令：

```bash
agent-pin health
agent-pin markdown --title "PR 审查结果" --file ./review.md
agent-pin markdown --title "结论" --text "第一版应该做成 Tauri 桌面 Pin。"
agent-pin image --title "装修效果图" --path ./render.png --caption "入门柜参考图"
agent-pin status --title "任务完成" --level success --text "审查完成：发现 2 个问题。"
agent-pin push --file ./pin.json
agent-pin list
agent-pin show <pinId>
agent-pin hide-all
```

Skill 应优先教 Agent 使用 CLI，而不是直接写 curl。

## 12. 窗口行为

每个 Pin 是一个独立窗口。

默认行为：

- 默认置顶
- 可拖动
- 可缩放
- 可关闭
- 内容可滚动
- 图片自动适配宽度
- 关闭不删除数据
- 多个 Pin 级联排列，避免完全重叠

默认尺寸：

```json
{
  "width": 420,
  "height": "auto",
  "alwaysOnTop": true
}
```

建议：

- 最小宽度：280
- 默认宽度：420
- 最大默认高度：屏幕高度的 70%

## 13. 托盘行为

应用启动后常驻系统托盘。

菜单：

- 显示最近 Pin
- 隐藏所有 Pin
- 打开 Agent Pin
- 打开数据目录
- 退出

MVP 可以没有完整主窗口，只保留托盘和 Pin 窗口。

## 14. 本地存储

MVP 不使用数据库，使用文件系统。

目录：

```text
~/.agent-pin/
  inbox/
  pins/
  failed/
  state.json
```

`state.json` 记录最近 Pin 和窗口状态。

## 15. 错误处理

任何坏输入都不能导致应用崩溃。

常见错误：

- `INVALID_JSON`：JSON 解析失败或未知 block type（serde 反序列化阶段拒绝）
- `INVALID_PIN_DOCUMENT`：version/title/blocks 非空/path 绝对路径/level 枚举校验失败
- `UNSUPPORTED_BLOCK_TYPE`：保留，当前未知 type 走 `INVALID_JSON`
- `IMAGE_NOT_FOUND`：保留，HTTP 层不校验文件存在性，前端 `<img>` onerror 兜底
- `IMAGE_UNSUPPORTED`：图片扩展名非 PNG/JPG/JPEG/WebP/GIF
- `WINDOW_CREATE_FAILED`：窗口创建失败
- `INTERNAL_ERROR`：内部错误

统一错误响应：

```json
{
  "ok": false,
  "error": {
    "code": "ERROR_CODE",
    "message": "Human readable message"
  }
}
```

## 16. 实现阶段

> **已由 `docs/phase-plan.md` 取代。**
>
> 本节保留的历史版本与最新分期口径不一致（历史版本把"系统托盘"放进 Phase 1，但最新口径把完整托盘能力放在 Phase 2，Phase 1 仅含最小托盘用于应用退出）。
>
> 实际分期以 `docs/phase-plan.md` 为准。本节不再维护，仅作历史参考。

## 17. 验收标准

- 应用可以启动
- 托盘可见
- `GET /api/health` 返回 ok
- `POST /api/pins` 创建独立 Pin 窗口
- `agent-pin markdown` 创建 Markdown Pin
- `agent-pin image` 创建 Image Pin
- `agent-pin status` 创建 Status Pin
- `agent-pin push --file` 创建混合 Pin
- 多 Pin 不完全重叠
- Pin 可以拖动、缩放、置顶、关闭
- 关闭后应用不退出
- 最近 Pin 可以从托盘重新打开
- 非法 JSON 不崩溃
- 图片路径不存在时显示错误
- 不包含 choice、事件回流、Artifact

## 18. 后续路线

后续版本可考虑：

- v0.2：透明度、锁定位置、边缘吸附、右键菜单
- v0.3：Choice Pin，只记录本地事件
- v0.4：事件回流，`agent-pin events`
- v0.5：HTTP 增强，更新/删除/查询 Pin
- v0.6：MCP Server
- v0.7：手动 Pin、剪贴板 Pin、截图 Pin

## 19. 关键原则

- 第一版只做纯展示
- 一个 Pin 是一个独立桌面窗口
- Pin 是容器，block 是内容
- Agent 优先通过 CLI 使用
- CLI 底层调用本地 HTTP
- 文件夹投递只是兜底
- 不要让 Agent 乱推 Pin
- 不要把聊天记录全文 pin 到桌面
- 每个 Pin 应该有明确用途
