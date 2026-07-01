# Changelog

本项目所有重要变更记录在此文件中。

格式基于 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循 [Semantic Versioning](https://semver.org/lang/zh-CN/)。

## [Unreleased]

### Added

- Pin 窗口最小尺寸约束（最小宽度 280px，最小高度 100px），对齐 docs/05_ui_style.md §4 的文档承诺。

### Changed

- `packages/shared` 的 `validate` 拆分 `MIN_WINDOW_DIMENSION` 为 `MIN_WINDOW_WIDTH`(280) / `MIN_WINDOW_HEIGHT`(100)，与窗口 `min_inner_size` 对齐。小于最小值的 width/height 会被 `validate` 拒绝（返回 `INVALID_PIN_DOCUMENT`），而非静默放大。

## [0.1.0] - 2026-06-30

### Added

- Tauri 2 桌面应用 + 系统托盘（应用存活指示 + Quit + 最近 5 个 hidden Pin 快恢 + 管理界面入口 + 隐藏全部）
- 本地 HTTP API，监听 `127.0.0.1:4317`：
  - `GET /api/health` — 健康检查
  - `POST /api/pins` — 创建 Pin 窗口
  - `GET /api/pins` — 列出所有 Pin 元数据
  - `POST /api/pins/{pinId}/show` — 重新显示已隐藏 Pin
  - `POST /api/pins/{pinId}/hide` — 隐藏 Pin
  - `POST /api/pins/hide-all` — 隐藏所有可见 Pin
- Rust `agent-pin` CLI（health / markdown / image / status / push / list / show / hide-all）
- Pin 支持 `markdown` / `image` / `status` 三种 block，可多 block 混排
- Pin 窗口支持拖动、缩放、置顶、关闭
- 文件系统持久化（`~/.agent-pin/pins/` + `state.json`），应用重启后历史保留
- 独立管理界面（完整历史列表 + 搜索 + 删除）
- 关闭 Pin 后可从托盘快恢或管理界面重新打开
- Agent Skill 文档（`skills/agent-pin/SKILL.md`）
- 更新检查（启动时静默检查 + 托盘菜单手动检查，24h 缓存）
- HTTP API 安全防护：CSRF 防护（Content-Type 校验 + Host 白名单）、请求体大小限制（1MB）、Pin 总数上限（500）
- CLI endpoint 校验（防 SSRF：仅允许本地回环地址，拒绝 https/userinfo/path/query/fragment）
- pin_id 路径穿越防护（拒绝 `/`、`\`、`..`、NUL 字节、保留 label `manager`）
- 单实例检查（重复启动时唤起已有实例，打开管理界面而非报端口绑定失败）
- 启动失败保护（HTTP 端口绑定失败 / 托盘构建失败时弹窗提示并优雅退出，不 panic）

### Security

- HTTP 服务只监听 `127.0.0.1`，不开放局域网
- 无身份验证、无 token、无远程访问能力（MVP 明确边界）
- 持久化文件未加密，路径为 `~/.agent-pin/`
