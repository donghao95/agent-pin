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
- 更新检查（调 GitHub API 查最新 release，托盘菜单入口）

Phase 1 托盘边界：只表示应用存活 + `Quit Agent Pin`，不做 Pin 历史恢复。关闭 Pin = 销毁窗口；托盘 Quit = 退出应用并停止 HTTP。

模块文件：

- `http.rs` — axum HTTP 服务，监听 `127.0.0.1:4317`
- `pin.rs` — PinDocument 数据模型（依赖 packages/shared）
- `pin_actions.rs` — Pin 操作编排（show / hide_all_visible），协调窗口创建/销毁与状态更新
- `registry.rs` — 内存 Pin 注册表，pinId 为键
- `storage.rs` — 持久化层（`~/.agent-pin/pins/` + `state.json`）
- `tray.rs` — 系统托盘菜单 + 事件处理
- `updater.rs` — 更新检查（GitHub API + 24h 缓存）
- `window.rs` — Pin 窗口创建/销毁
- `lib.rs` — 应用入口 + invoke 命令 + setup hook

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

assetProtocol scope 安全说明：

- `tauri.conf.json` 中 `assetProtocol.scope` 当前为 `["**"]`，允许加载系统任意路径图片。
- 这是 MVP 功能需要：Agent 通过 HTTP 传入任意绝对路径，前端 `convertFileSrc(path)` 直接走 Tauri asset protocol，无需先复制文件。
- 收窄 scope 到 `~/.agent-pin/images/**` 会破坏 docs/01_product_spec.md §8 "image path 必须是绝对路径" 的 MVP 契约。
- 风险：本地任意用户进程均可通过 HTTP 接口触发任意路径图片加载（受 `127.0.0.1` 监听 + 本地用户权限约束）。
- Roadmap：未来若需收窄 scope，CLI 先把图片复制到 `~/.agent-pin/images/` 下再传本地路径，前端只引用该目录。记为 roadmap，不在 MVP 实现。

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

Phase 2-B 有三套入口触发 Pin 状态变更，它们的状态更新和事件广播职责如下：

状态变更后由 `registry` 统一 emit `pins:changed` 事件，托盘和管理界面各自 `listen` 该事件自动刷新，调用方不再需要手动调 `tray::refresh`。这是发布订阅模式：registry 是事件源，视图是订阅者。

| 入口 | 状态更新 | 事件广播 | 说明 |
|------|---------|---------|------|
| HTTP `/api/pins/{pinId}/show` | `set_state(visible)`，失败则回滚 destroy 窗口 | registry emit `pins:changed` | HTTP 是 Agent 的核心入口 |
| HTTP `/api/pins/{pinId}/hide` | `set_state(hidden)` | registry emit `pins:changed` | |
| HTTP `/api/pins/hide-all` | 遍历 visible 逐个 `set_state_quiet(hidden)`，循环结束统一 emit 一次 | registry emit `pins:changed` ×1 | 批量用 quiet 避免 N 次托盘重建 |
| invoke `show_pin` / `hide_pin` / `hide_all_pins` / `delete_pin` | 同 HTTP 对应路由 | 同 HTTP 对应路由 | 管理界面按钮触发 |
| 托盘快恢菜单点击 | `show_pin_by_id` 内 `set_state(visible)`，失败回滚 | registry emit `pins:changed` | |
| 窗口关闭按钮 → `WindowEvent::Destroyed` | `on_window_event` 检测到后 `set_state(hidden)` | registry emit `pins:changed` | 只在 state==visible 时更新，避免与 HTTP hide 重复 |

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
registry emit `pins:changed`
  ↓
托盘 listen 收到事件 → refresh() 重建菜单（该 Pin 进入最近 5 hidden 列表）
管理界面 listen 收到事件 → 刷新列表
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
set_state(visible) → registry emit `pins:changed`
  ↓
托盘/管理界面 listen 收到事件自动刷新
```

### Phase 2-B：删除 Pin

```text
管理界面点"删除" + window.confirm 确认 / invoke delete_pin
  ↓
hide_pin_window 销毁窗口（如果存在）
  ↓
registry.remove(pinId)：
  - 从内存 registry 移除 + 更新 state.json（同一锁内，失败则回滚内存）
  - 释放锁后删 pins/{pinId}.json 文件（best-effort，失败仅日志）
  - registry emit `pins:changed`
  ↓
托盘/管理界面 listen 收到事件自动刷新
```

删除不可恢复，与 hide（可恢复）是两套独立路径。

### Phase 2-B：事件机制（发布订阅）

Pin 状态变更后需要通知所有视图（托盘菜单、管理界面列表）刷新。采用发布订阅模式：`registry` 是事件源，视图是订阅者，解耦状态管理与视图刷新。

设计理由：旧设计由各调用点手动调 `tray::refresh()`，容易遗漏（`create_pin` 就漏过），且 `tray::refresh` 职责过载（既管托盘菜单，又被当作状态广播入口）。新设计由 registry 在状态变更成功后统一 emit 事件，调用方不再需要手动刷新视图。

#### 事件列表

| 事件名 | 载荷 | 触发时机 | 订阅者 |
|--------|------|---------|--------|
| `pins:changed` | `()`（无载荷） | registry `insert` / `set_state` / `remove` 成功后 | 托盘（refresh 重建菜单）、管理界面（refresh 刷新列表） |
| `pin:show-failed` | `{ pinId: string, message: string }` | `show_pin` AsyncCreate 模式下窗口异步创建失败时 | 管理界面（显示错误 + refresh） |

#### registry 事件 API

- `emit_changed()`：模块级公开函数，emit `pins:changed`。供批量操作调用。
- `set_state()`：更新状态 + 自动 emit。
- `set_state_quiet()`：更新状态不 emit。供批量操作循环内使用，循环结束后调用方统一 `emit_changed()` 一次。
- `insert()` / `remove()`：内部成功后自动 emit。

#### 批量操作 emit 策略

`hide_all_visible` 遍历所有 visible Pin 逐个 `set_state_quiet(Hidden)`，循环结束后统一调用 `emit_changed()` 一次。避免 N 次 emit 触发 N 次同步托盘菜单重建（Tauri 2 emit 是同步派发）。

#### 死锁防护

registry 的 `insert` / `set_state_inner` / `remove` 在 emit 前必须 `drop(inner)` 释放 Mutex 锁。原因：Tauri 2 的 `emit` 是同步派发，订阅者的 handler（如 `tray::refresh`）会尝试 `REGISTRY.list()` 加锁，若 emit 时持锁会死锁。

#### 启动时序

`set_app_handle()` 必须在 `load_from_disk()` 之前调用（lib.rs setup 第一步）。原因：`load_from_disk` 不 emit 事件（启动时无订阅者），但后续的 `insert`/`set_state`/`remove` 需要 AppHandle 来 emit。若 `set_app_handle` 在 `load_from_disk` 之后，`load_from_disk` 内部若有未来改动触发 emit，APP_HANDLE 为 None 会静默跳过。

#### 管理界面 debounce

Manager.tsx 对 `pins:changed` 监听加 50ms debounce：50ms 内多次 emit 只 refresh 一次。防御批量操作或短时间内多个状态变更（如连续创建多个 Pin）触发多次列表刷新。

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

窗口行为差异：

- **Pin 窗口关闭**：destroy 窗口 + state=hidden（记录保留，可从托盘/管理界面恢复）。Phase 2-B 设计。
- **管理界面窗口关闭**：拦截 `CloseRequested`，改为 `hide()`（窗口实例保留，不 destroy）。用户点托盘"打开管理界面"重新 `show()`。只有托盘"退出 Agent Pin"（`app.exit(0)`）才真正退出 app。
- **应用启动**：setup 完成后自动打开管理界面窗口，用户启动即可见。

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
- 删除 Pin：先更新 `state.json`（内存移除 + 持久化，同一锁内失败回滚），再删 `pins/{pinId}.json`（best-effort，失败仅日志）

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
