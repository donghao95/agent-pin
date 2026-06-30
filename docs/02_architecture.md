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

- 最小系统托盘（Phase 1 仅应用退出；Phase 2 起 扩展为历史恢复入口）
- 本地 HTTP 服务
- Pin 窗口创建和管理
- 最近 Pin 状态保存，Phase 2
- 渲染 Pin 内容

Phase 1 托盘边界：只表示应用存活 + `Quit Agent Pin`，不做 Pin 历史恢复。关闭 Pin = 销毁窗口；托盘 Quit = 退出应用并停止 HTTP。

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

### Phase 2-A：image block 渲染

```text
POST /api/pins (含 image block, path 是绝对路径)
  ↓
Desktop 校验 PinDocument（含扩展名白名单）
  ↓
创建 Pin 窗口
  ↓
前端 get_pin_document 取数据
  ↓
前端 convertFileSrc(path) 把绝对路径转 Tauri asset protocol URL
  (Windows/Linux: http://asset.localhost/<encoded-path>；macOS: asset://localhost/<path>)
  ↓
WebView 通过 Tauri asset protocol 加载本地图片
  ↓
图片不存在/格式不支持 → <img> onerror → 显示错误块
```

image path 规则：

- HTTP 接收的 `path` 必须是绝对路径。
- 扩展名必须是 PNG/JPG/JPEG/WebP/GIF 之一（`ImageUnsupported` 错误）。
- 不校验文件存在性：desktop 不知道 Agent cwd，前端 `<img>` onerror 处理。
- 相对路径解析在 Phase 2-C CLI 实现：CLI 把相对路径转绝对再 POST。
- 直接用 curl 测试时，调用者需传绝对路径。

### Phase 2-C：CLI 创建 Pin

```text
agent-pin markdown --title "PR 审查结果" --file review.md
  ↓
CLI 读取 review.md
  ↓
CLI POST /api/pins
  ↓
Desktop 校验 PinDocument（持久化由 Phase 2-B 已实现）
  ↓
创建 Pin 窗口
```

### Phase 2-B：入口与状态更新职责

Phase 2-B 有三套入口触发 Pin 状态变更，它们的状态更新和托盘刷新职责如下：

| 入口 | 状态更新 | 托盘刷新 | 说明 |
|------|---------|---------|------|
| HTTP `/api/pins/{pinId}/show` | `set_state(visible)`，失败则回滚 destroy 窗口 | `tray::refresh` | HTTP 是 Agent 的核心入口 |
| HTTP `/api/pins/{pinId}/hide` | `set_state(hidden)` | `tray::refresh` | |
| HTTP `/api/pins/hide-all` | 遍历 visible 逐个 `set_state(hidden)` | `tray::refresh` | |
| invoke `show_pin` / `hide_pin` / `hide_all_pins` / `delete_pin` | 同 HTTP 对应路由 | `tray::refresh` | 管理界面按钮触发 |
| 托盘快恢菜单点击 | `show_pin_by_id` 内 `set_state(visible)`，失败回滚 | 调用方 `refresh` | |
| 窗口关闭按钮 → `WindowEvent::Destroyed` | `on_window_event` 检测到后 `set_state(hidden)` | `tray::refresh` | 只在 state==visible 时更新，避免与 HTTP hide 重复 |

关键约束：`window.rs create_pin_window` 要求 label（pinId）不冲突。所有 show 入口在调 `create_pin_window` 前必须先调 `hide_pin_window` 清理可能的孤儿窗口。

### Phase 2-B：关闭 Pin

```text
用户点 Pin 窗口关闭按钮 / POST /api/pins/{pinId}/hide / 托盘"隐藏全部"
  ↓
窗口 destroy（WindowEvent::Destroyed 触发）
  ↓
lib.rs on_window_event 检测到 Destroyed：
  - 若 label == "manager" 忽略
  - 若 registry 中该 Pin 当前 state == visible，则 set_state(hidden)
  - 若已是 hidden（HTTP/invoke hide 主动触发，先于事件回调执行），不重复更新
  ↓
state.json 更新 state=hidden, updatedAt=now
  ↓
托盘 refresh() 重建菜单（该 Pin 进入最近 5 hidden 列表）
  ↓
管理界面仍保留记录
```

### Phase 2-B：重新打开 Pin

```text
用户从托盘右键最近 5 hidden Pin 点击 / 管理界面点"显示" / POST /api/pins/{pinId}/show
  ↓
registry.get(pinId) 从内存读 PinDocument（启动时已 load_from_disk）
  ↓
清理可能的孤儿窗口（hide_pin_window 幂等）
  ↓
create_pin_window 重新创建窗口
  ↓
set_state(visible), 托盘 refresh()
```

### Phase 2-B：删除 Pin

```text
管理界面点"删除" + window.confirm 确认 / invoke delete_pin
  ↓
hide_pin_window 销毁窗口（如果存在）
  ↓
registry.remove(pinId)：
  - 删 pins/{pinId}.json 文件
  - 从内存 registry 移除
  - 更新 state.json
  ↓
管理界面 refresh()
```

删除不可恢复，与 hide（可恢复）是两套独立路径。

---

## 4. 窗口管理

每个 Pin 是一个独立窗口。

窗口 label（即 pinId）格式：

```text
pin_<timestamp_ms>_<6位随机数字>
```

例如 `pin_1782801843675_717272`。timestamp_ms 是 Unix 毫秒时间戳，6 位随机用于同一毫秒内的冲突避免。

窗口类型：

- **Pin 窗口**（label 是 pinId）：`decorations(false)` + `shadow(true)` + 自定义轻标题栏 + `alwaysOnTop=true` + `skipTaskbar=true`
- **管理界面窗口**（label 固定为 `manager`）：`decorations(true)` 系统装饰 + `resizable(true)` + 880×620 + min 640×400

窗口创建默认参数（Pin 窗口）：

- alwaysOnTop: true
- resizable: true
- decorations: false（前端自定义标题栏）
- shadow: true（DWM 提供 OS 级圆角+阴影）
- skipTaskbar: true

MVP 不做复杂窗口吸附和透明度。

---

## 5. 存储

Phase 1 不实现历史存储，只保证窗口可创建。

Phase 2-B 实现文件系统存储：

```text
~/.agent-pin/           # Windows: %USERPROFILE%\.agent-pin\
  pins/                 # 每个 Pin 的 PinDocument，文件名 {pinId}.json
  state.json            # 所有 Pin 的元数据（PinMeta 列表）
```

`state.json` 结构：

```json
{
  "version": 1,
  "pins": [
    {
      "pinId": "pin_1782801843675_717272",
      "title": "PR 审查结果",
      "createdAt": "2026-06-30T12:15:30+08:00",
      "updatedAt": "2026-06-30T12:20:00+08:00",
      "state": "visible",
      "source": { "agent": "codex", "workspace": "TryCue" }
    }
  ]
}
```

写入策略：

- 创建 Pin：先写 `pins/{pinId}.json`，再更新 `state.json`（原子写：先写 `.tmp` 再 rename）
- 状态变更（show/hide）：只更新 `state.json` 的 `state` 和 `updatedAt` 字段
- 删除 Pin：先删 `pins/{pinId}.json`，再更新 `state.json`

启动加载（`load_from_disk`）：

- 读取 `state.json`，坏文件降级为空列表（不阻塞启动）
- 每个 Pin 的 `state` 如果是 `visible`，降级为 `hidden`（重启后窗口实际不可见）
- 对应的 `pins/{pinId}.json` 缺失或损坏时，标记为 `failed`，并写入占位 PinDocument
- 不自动恢复窗口（用户主动 show）

历史能力属于完整 MVP：Pin 关闭后不能直接消失，必须可恢复（Phase 2-B 实现）。

---

## 6. 前端风格

MVP 视觉目标：轻、克制、像桌面工具，不像网页后台或数据大屏。

默认风格（MVP 长期目标）：

- 浅色优先
- 圆角卡片
- 柔和阴影
- 极简标题栏
- 内容区域留白充足
- Markdown 阅读体验优先
- 图片展示干净
- 可适度使用半透明或轻毛玻璃，但不能影响可读性

Phase 1 视觉退化（Windows WebView2 限制，详见 `docs/05_ui_style.md` §4）：

- CSS 不做圆角/阴影/毛玻璃（透明背景 + 圆角会露黑边，backdrop-filter 缩放抖动）
- 窗口用 `WebviewWindowBuilder::shadow(true)` 让 DWM 提供 OS 级圆角+阴影（Win11 有，Win10 退化直角）
- 移动/缩放时 WebView2 重绘延迟的边缘闪烁属已知限制，Phase 1 接受

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
