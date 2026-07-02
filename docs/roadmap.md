# Agent Pin 未来功能路线

本文件记录未落地、正在讨论或属于未来路线的功能。
已实现并稳定的功能不写在这里，写在对应的规格文档中。

## 1. 用户编辑 Pin 内容并保存

状态：未来路线，不在当前 MVP 范围

### 背景

当前 Pin 窗口的 select-mode 是只读的：用户可滑动选择文本复制、右键复制图片，但不能修改 Pin 内容并保存。

### 为什么暂不做

AGENTS.md 核心不变量：

> MVP 只做 Agent → App 的纯展示，不做 User → Agent 事件回流。

用户编辑 Pin 内容并保存会引入"用户回流"概念：
- 编辑后的内容如何同步回 Agent？
- 是否需要版本管理？
- 是否需要冲突解决？

这些问题超出 MVP 边界，会破坏"轻量、纯展示"定位。

### 未来方向

- 编辑后的内容作为本地覆盖层保存，不影响 Agent 推送的原始内容。
- 提供差异视图，让用户看到 Agent 原始内容 vs 本地编辑。
- 可选的"回滚到原始内容"操作。

## 2. HTML block / 完整 HTML Artifact

状态：未来路线，不在当前 MVP 范围

### 背景

当前 Pin 支持 `markdown` / `image` / `status` 三种 block。讨论是否引入 `html` block 让 Agent 直接推送 HTML 内容，以获得更丰富的表达力（复杂布局、内嵌图表等）。

### 为什么暂不做

1. **与 MVP 不变量冲突**：`docs/01_product_spec.md` §4 明确"不做完整 HTML Artifact"。HTML block 本质是简化版 Artifact，破坏"纯展示 MVP、轻量、本地优先"边界。

2. **契约逃生舱效应**：一旦提供 `html` block，Agent 遇到任何 markdown 不擅长的场景都会 fallback 到 HTML，导致 markdown 路径萎缩，最终 Agent Pin 退化为"简化版浏览器"。

3. **视觉一致性破坏**：Agent 自带 inline style 会破坏 `docs/05_ui_style.md` 的"轻量桌面贴纸"视觉不变量。Markdown 让 Pin 主题统一控制视觉，HTML 不可控。

4. **窗口自适应高度冲突**：`docs/01_product_spec.md` §9.2 的 `fit_pin_window_height` 机制依赖 `.pin-body` 的 `scrollHeight` 测量。sandbox iframe 的内容高度测量是经典难题，会破坏当前自适应高度链路。

5. **安全成本**：必须引入 sanitize（如 DOMPurify）或 sandbox iframe（`sandbox="allow-same-origin"` 不给 `allow-scripts`），同时需要调整当前 CSP 配置。这是非平凡的工程成本。

6. **markdown + image 已覆盖 MVP 95% 场景**：PR 审查结论、状态提醒、任务摘要、风险提示等核心场景 markdown 足够。真不够表达时，image block 是更安全的逃生舱 —— Agent 用其他工具生成图，Pin 只展示。

### 未来方向

如果未来确实有强烈需求（例如带图表的报表、复杂卡片布局）：

- **独立立项**：作为 Phase 3+ 功能，不塞进 MVP。
- **沙箱隔离**：用 sandbox iframe 渲染，不给 `allow-scripts`，避免脚本注入。
- **样式隔离**：iframe 自带样式边界，不污染 Pin 主题。
- **同步更新 7 处契约**：`01_product_spec.md` / `03_api.md` / `04_cli.md` / `07_skill_design.md` / `skills/agent-pin/SKILL.md` / `examples/pins/` / `02_architecture.md` / `05_ui_style.md`。
- **接受兼容成本**：明确 HTML block 不参与 `fit_pin_window_height` 自适应，由 Agent 显式指定 `window.height`，或固定 iframe 高度。
- **优先评估替代方案**：先考虑扩展 markdown（如 Mermaid 数学公式、LaTeX）能否满足需求，再考虑引入 HTML block。

## 3. 其他未来功能

- 透明度调节
- 锁定位置
- 吸附边缘
- 主题切换（深色模式）
- Pin 标签、分类、搜索（增强版）
- WebSocket 实时状态更新
- 云同步和账号系统
- 截图、OCR、录屏
- 图片编辑
- MCP Server
- 网页分享
