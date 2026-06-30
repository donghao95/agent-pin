# Agent Pin 架构草案

## 1. MVP 架构

```text
Agent / Skill
  ↓
agent-pin CLI
  ↓
HTTP API: http://127.0.0.1:4317
  ↓
Tauri Rust Backend
  ↓
Pin Window Manager
  ↓
Web Frontend 渲染 Pin
```

## 2. 模块

### apps/desktop

Tauri 桌面应用。

职责：

- 系统托盘
- 本地 HTTP 服务
- Pin 窗口创建和管理
- 最近 Pin 状态保存
- 渲染 Pin 内容

### packages/cli

`agent-pin` CLI。

职责：

- 读取命令行参数
- 读取 Markdown 文件或图片路径
- 组装 Pin JSON
- 调用本地 HTTP API

### packages/shared

共享类型和校验逻辑。

职责：

- PinDocument 类型
- PinBlock 类型
- JSON schema / zod schema
- 错误码定义

MVP 可以先不强制抽 shared 包，但后续建议抽离，避免 CLI 和 desktop 类型漂移。

## 3. 数据流

### 创建 Pin

```text
agent-pin markdown --title "PR 审查结果" --file review.md
  ↓
CLI 读取 review.md
  ↓
CLI POST /api/pins
  ↓
Desktop 校验 PinDocument
  ↓
保存到 ~/.agent-pin/pins/
  ↓
创建 Pin 窗口
```

### 关闭 Pin

```text
用户关闭窗口
  ↓
窗口隐藏或销毁
  ↓
state.json 更新 visible=false
  ↓
托盘最近列表仍保留
```

### 重新打开 Pin

```text
用户从托盘点击最近 Pin
  ↓
读取 pins/<pinId>.json
  ↓
重新创建窗口
```

## 4. 窗口管理

每个 Pin 是一个独立窗口。

窗口 label 建议：

```text
pin_<timestamp>_<slug>
```

窗口创建默认参数：

- alwaysOnTop: true
- resizable: true
- decorations: false 或轻边框
- skipTaskbar: true，可选

MVP 不做复杂窗口吸附和透明度。

## 5. 存储

默认目录：

```text
~/.agent-pin/
  inbox/
  pins/
  failed/
  state.json
```

MVP 不使用数据库。

## 6. 后续扩展点

- choice pin
- 事件队列
- `agent-pin events`
- MCP server
- 手动 Pin / 剪贴板 Pin
- 截图 Pin
