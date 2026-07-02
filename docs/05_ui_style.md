# Agent Pin UI 风格说明

## 1. 风格方向

Agent Pin MVP 的视觉方向：接近 PixPin 和 ChatGPT App 的结合。

更具体地说：

- 像 PixPin：轻量、贴在桌面上、像一个可以被移动和关闭的桌面贴纸。
- 像 ChatGPT App：干净、圆润、克制、阅读舒适、不像传统后台系统。

Pin 应该像一个轻量桌面贴纸，而不是完整应用窗口。

## 2. 默认视觉原则

- 浅色优先。
- 圆角卡片。
- 柔和阴影。
- 极简标题栏（无独立标题栏，title 融入内容首行）。
- 内容区域留白充足。
- Markdown 阅读体验优先。
- 图片展示干净。
- 可适度使用半透明或轻毛玻璃，但不能影响可读性。
- 交互反馈要轻，不做夸张动画。

注：以上是 MVP 长期目标。Phase 1 受 Windows WebView2 透明窗口限制，圆角/阴影/毛玻璃的 CSS 实现会引发黑边和缩放抖动，Phase 1 采用退化方案（见 §4）。

## 3. 避免的方向

不要做成：

- 深色控制台风。
- 厚重科技蓝。
- 大屏数据看板风。
- 复杂动画。
- 类网页 dashboard。
- 过度拟物。
- IDE 面板。
- 浏览器网页卡片。

## 4. Pin 窗口 UI 细节

Pin 窗口按以下方式实现：

- 默认出现在屏幕右上角。
- 多个 Pin 级联向左下偏移。
- 默认宽度 420px。
- 最小宽度 280px。
- 最小高度 100px。
- 最大默认高度为屏幕高度的 70%。
- 内容超过最大高度时内部滚动，底部渐变透明暗示下方还有内容。
- 默认 `alwaysOnTop = true`。
- **无独立标题栏**：title 融入内容首行（`.pin-title-inline`），关闭按钮悬浮在右上角（`.pin-close-floating`，hover 显示）。
- 整窗可拖动（view-mode 下 `onMouseDown` → `startDragging()`）。
- 双击或右击进入只读选择模式（select-mode）：可滑动选择文本、右键复制图片；ESC 退出。右击时弹出上下文菜单（只读模式 / 隐藏 Pin）。
- ESC 在 view-mode 下关闭窗口（hidden，可恢复）。
- 窗口高度按内容自适应（`height: "auto"` 时）：初始 200px，前端测量后回流调整。
- 用户手动 resize 后尺寸记忆到 `PinMeta.window_size`，重新打开时恢复，自动适配不再覆盖（见 §4.2）。
- 最小尺寸 280×100 由 `min_inner_size` 强制，不受内容影响。

### 4.1 交互模式

Pin 窗口有两种交互模式（通过 `.pin-root` 上的 class 切换）：

- **view-mode**（默认）：
  - 整窗可拖动，`user-select: none`
  - 双击 → 进入 select-mode
  - 右击 → 弹出上下文菜单（只读模式 / 隐藏 Pin），`preventDefault` 抑制原生网页菜单
  - 链接点击用系统默认浏览器打开（Tauri WebView 不自动打开外链，需 `invoke open_external_url`）
  - ESC → 关闭窗口（`invoke close_pin`，同步 state=hidden + 管理页刷新）
- **select-mode**（只读选择模式）：
  - `user-select: text`，可滑动选择文本复制
  - 不触发拖动
  - 图片可右键复制
  - 顶部显示提示条"只读模式 · 按 ESC 退出"
  - 右上角 × 按钮语义变为"退出只读模式"（模式嵌套：从哪进从哪出，不跳级关闭窗口）
  - ESC → 退回 view-mode（优先于关闭窗口）

MVP 边界：select-mode 只读，永不支持修改 Pin 内容并保存（见 `docs/roadmap.md`）。

### 4.2 窗口尺寸记忆（2026-07-02 引入）

用户手动 resize 窗口后，尺寸持久化到 `PinMeta.window_size`（`state.json`），重新打开时优先恢复。

尺寸优先级（高 → 低）：

1. 用户记忆尺寸（`PinMeta.window_size`）
2. `PinDocument.window` 配置（Agent 通过 API/CLI 指定）
3. 默认值（420 × 600，`height: "auto"` 时初始 200px）

自动适配与记忆的协调：

- 前端 `userResized` 标志：用户手动 resize 后置 true，`measureAndFit` 不再调 `fit_pin_window_height`（只更新溢出状态）
- 后端 `fit_pin_window_height` 守卫：`PinMeta.window_size` 存在时返回 `Ok(false)`，不 `set_size`（双重保护）
- 前端 `programResizing` 标志：`measureAndFit` 调 invoke 前置 true，用于 `onResize` 区分程序 resize 和用户 resize
- 用户 resize 后 300ms 防抖调 `invoke remember_pin_size` 持久化

最小尺寸 280×100 由 `min_inner_size` 强制，用户手动缩放时不受内容影响。

### 4.3 视觉退化说明（Windows WebView2 限制）

- 窗口背景：不透明白色（`#ffffff`），不做透明/毛玻璃。透明背景 + 圆角会露出窗口黑色底，且 `backdrop-filter` 在缩放时重绘抖动。
- 圆角：CSS 不做圆角。依赖 `WebviewWindowBuilder::shadow(true)` 让 DWM 提供 OS 级圆角——Windows 11 有圆角，Windows 10 退化为直角。
- 阴影：同上，由 DWM 提供 OS 级阴影，Win10 可能无阴影。
- 渐变透明暗示：用 `.pin-root.show-fade::after` 叠层实现（`linear-gradient(transparent, #ffffff)`），不用 `backdrop-filter` 避免缩放抖动，不用 `mask-image` 避免遮罩滚动条。
- 残留问题：窗口移动/缩放时 WebView2 重绘延迟仍可能导致边缘闪烁，Phase 1 接受，Phase 2 评估 transparent 窗口或其他合成方案。

Phase 1 验收平台前提：视觉验收以 Windows 11 为准；Windows 10 退化为直角白矩形，属可接受退化。

## 5. Phase 1 暂不做

Phase 1 不做：

- 透明度调节。
- select-mode 下的自定义右键菜单（图片右键复制由浏览器原生支持；view-mode 右键菜单已实现，见 §4.1）。
- 锁定位置。
- 吸附边缘。
- 主题切换。
- 历史列表 UI。
- 复杂动效。
- 用户编辑 Pin 内容并保存（见 `docs/roadmap.md`）。

## 6. 设计验收标准

Pin 窗口应满足：

- 看起来像桌面贴纸，而不是后台系统窗口（Win11：DWM 圆角+阴影；Win10：直角白矩形，可接受退化）。
- 内容阅读舒适。
- 无标题栏干扰，title 融入内容但不抢视觉。
- 关闭按钮 hover 显示，清晰但不突兀。
- Markdown 标题、列表、代码块在小窗口中可读。
- 置顶时不显得打扰。
- 窗口高度按内容自适应，不出现大块空白。
- 用户手动 resize 后尺寸记忆，重新打开恢复原样，最小 280×100 不受内容影响。
- 内容溢出时底部渐变透明暗示，滚动到底部后渐变消失。
- 双击进入只读模式，可选中文本复制，ESC 退出。右击弹出上下文菜单（只读模式 / 隐藏 Pin），不出现原生网页菜单。
- ESC 关闭后管理页列表立即刷新（同步 state=hidden + pins:changed 事件）。
- 移动/缩放时的边缘闪烁属已知限制，不阻塞验收。

## 7. 管理界面窗口（Phase 2-B）

Phase 2-B 引入独立的管理界面窗口（`?manager=1`），与 Pin 窗口视觉风格独立。

技术差异：

- 使用系统装饰（`decorations(true)`），由 OS 提供标题栏、关闭/最大化/最小化按钮。
- 不受 Pin 窗口的 WebView2 透明窗口限制，可自由使用 CSS 圆角、阴影、半透明背景。
- 默认尺寸 880×620，最小 640×400，可调整大小。

视觉原则：

- 浅色主题，与 Pin 窗口风格呼应（都是"轻、克制"的桌面工具感）。
- 顶部 header：标题 + 统计（共/可见/隐藏/异常）+ 搜索框 + 工具按钮（刷新/隐藏全部/打开数据目录）。
- 列表区：PinCard 卡片式排列，每张卡片显示状态图标 + 标题 + 时间 + agent/workspace 标签 + 显示/隐藏/删除按钮。
- 状态图标：visible=●（实心圆）、hidden=○（空心圆）、failed=✕。
- 删除操作前 `window.confirm` 二次确认。
- 错误消息以可点击关闭的提示条展示，不阻塞列表操作。

类名前缀：

- 布局类：`manager-*`（如 `manager-root`、`manager-header`、`manager-body`）。
- 卡片类：`pin-card-*`（如 `pin-card`、`pin-card-title`、`pin-card-btn`）。

注意：`pin-card-*` 类名与 Pin 窗口的 `pin-*` 类名不重叠（Pin 窗口没有 `pin-card` 前缀），两个 CSS 文件可同时加载无冲突。
