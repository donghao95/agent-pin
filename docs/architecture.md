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

Phase 1 先跳过 CLI，直接通过 HTTP 验证最小闭环：

```text
curl / HTTP client
  ↓
POST /api/pins
  ↓
Tauri Rust Backend
  ↓
创建 Markdown Pin Window
```

Phase 2 再补 Rust CLI、历史、托盘恢复和完整 blocks。

---

## 2. 模块

### apps/desktop

Tauri 桌面应用。

职责：

- 系统托盘，Phase 2
- 本地 HTTP 服务
- Pin 窗口创建和管理
- 最近 Pin 状态保存，Phase 2
- 渲染 Pin 内容

### packages/cli

Rust `agent-pin` CLI，Phase 2 实现。

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
- JSON schema / 校验逻辑
- 错误码定义

MVP 建议尽早抽 shared，避免 CLI 和 desktop 类型漂移。

---

## 3. 数据流

### Phase 1：创建 Markdown Pin

```text
curl POST /api/pins
  ↓
Desktop 校验 PinDocument
  ↓
创建独立 Pin 窗口
  ↓
前端渲染 Markdown
```

### Phase 2：CLI 创建 Pin

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

### Phase 2：关闭 Pin

```text
用户关闭窗口
  ↓
窗口隐藏或销毁
  ↓
state.json 更新 visible=false
  ↓
托盘最近列表仍保留
```

### Phase 2：重新打开 Pin

```text
用户从托盘点击最近 Pin
  ↓
读取 pins/<pinId>.json
  ↓
重新创建窗口
```

---

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

---

## 5. 存储

Phase 1 可以不实现历史存储，只保证窗口可创建。

Phase 2 必须实现文件系统存储：

```text
~/.agent-pin/
  inbox/
  pins/
  failed/
  state.json
```

历史能力属于完整 MVP：Pin 关闭后不能直接消失，必须可恢复。

---

## 6. 前端风格

MVP 视觉目标：轻、克制、像桌面工具，不像网页后台或数据大屏。

默认风格：

- 浅色优先
- 圆角卡片
- 柔和阴影
- 极简标题栏
- 内容区域留白充足
- Markdown 阅读体验优先
- 图片展示干净
- 可适度使用半透明或轻毛玻璃，但不能影响可读性

避免：

- 深色控制台风
- 厚重科技蓝
- 大屏数据看板风
- 复杂动画
- 类网页 dashboard
- 过度拟物

---

## 7. 后续扩展点

- choice pin
- 事件队列
- `agent-pin events`
- MCP server
- 手动 Pin / 剪贴板 Pin
- 截图 Pin
