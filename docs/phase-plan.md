# Agent Pin MVP 分期计划

本文件记录当前最新执行口径。若与 `docs/mvp-spec.md` 早期描述存在差异，以本文件为准。

## Phase 1：最小闭环

目标：先验证 Agent Pin 最核心体验。

```text
HTTP 请求 → 创建独立 Markdown Pin 窗口
```

必须做：

- Tauri 2 桌面应用
- 本地 HTTP 服务，监听 `127.0.0.1:4317`
- `GET /api/health`
- `POST /api/pins`
- 接收 `markdown` block
- 每个 Pin 是独立桌面窗口
- Pin 窗口可拖动、缩放、置顶、关闭
- 基础错误处理，坏输入不能导致应用崩溃

Phase 1 暂不做：

- CLI
- image block
- status block
- 多 block 混排
- 最近 Pin 历史
- 托盘恢复
- `GET /api/pins`
- `show` / `hide` / `hide-all`

验收命令：

```bash
curl -X POST http://127.0.0.1:4317/api/pins \
  -H "Content-Type: application/json" \
  -d '{"version":1,"title":"测试 Pin","blocks":[{"type":"markdown","content":"## Hello Agent Pin\n这是第一个 Pin。"}]}'
```

预期：桌面出现一个独立 Markdown Pin 窗口。

## Phase 2：完整 MVP

Phase 2 补齐完整 MVP 能力。

必须做：

- 系统托盘
- Rust `agent-pin` CLI
- `markdown` / `image` / `status` blocks
- 一个 Pin 可以包含多个 block
- Markdown 支持标题、列表、引用、代码块、链接、Markdown 表格
- Image 支持本地图片路径
- Status 支持 `info` / `success` / `warning` / `error`
- 最近 Pin 简单历史
- 关闭后可从托盘重新打开
- `GET /api/pins`
- `POST /api/pins/:pinId/show`
- `POST /api/pins/:pinId/hide`
- `POST /api/pins/hide-all`
- Agent 使用 skill 文档

历史能力属于完整 MVP：Pin 关闭后不能直接消失，必须保留记录并可恢复。

## CLI 技术决策

CLI 直接使用 Rust 实现，不再使用 Node.js MVP CLI。

理由：

- Agent Pin 是本地桌面工具，Rust CLI 更符合长期方向。
- 可以和 Tauri backend 复用类型、schema 和错误码。
- 避免后续从 Node.js CLI 迁移到 Rust CLI。

## 前端视觉口径

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

Pin 应该像一个轻量桌面贴纸，而不是完整应用窗口。
