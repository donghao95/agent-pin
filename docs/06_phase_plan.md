# Agent Pin 分期计划与验收

版本：v0.1
阶段：MVP（Phase 1 + Phase 2 已实现）
状态：稳定

本文件记录当前最新执行口径。若与早期文档描述存在差异，以本文件为准。

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
- 最小系统托盘：仅表示应用仍在运行，并提供 `Quit Agent Pin` 退出应用

Phase 1 暂不做：

- CLI
- image block
- status block
- 多 block 混排
- 最近 Pin 历史
- 托盘的 Pin 历史恢复（Phase 1 托盘不做 Pin 生命周期管理，关闭 Pin = 销毁窗口，不承诺恢复）
- `GET /api/pins`
- `show` / `hide` / `hide-all`

Phase 1 托盘边界说明：

- 托盘只负责应用生命周期（显示应用存活 + 退出应用）。
- 托盘不负责 Pin 生命周期（不做历史列表、不做恢复、不做 hide-all）。
- 关闭 Pin 窗口 = 销毁该窗口；托盘 Quit = 退出应用并停止 HTTP 服务。

验收命令：

```bash
curl -X POST http://127.0.0.1:4317/api/pins \
  -H "Content-Type: application/json" \
  -d '{"version":1,"title":"测试 Pin","blocks":[{"type":"markdown","content":"## Hello Agent Pin\n这是第一个 Pin。"}]}'
```

预期：桌面出现一个独立 Markdown Pin 窗口。

## Phase 2：完整 MVP

Phase 2 补齐完整 MVP 能力。拆分为 4 个子阶段，按顺序推进。

### Phase 2-A：Block 扩展

必须做：

- `image` block（本地图片绝对路径）
- `status` block（`info` / `success` / `warning` / `error`）
- 一个 Pin 可以包含多个 block 混排
- Markdown 支持标题、列表、引用、代码块、链接、Markdown 表格
- 图片路径不存在时，在 Pin 内显示错误块，不崩溃
- image block 的 `path` 必须是绝对路径（相对路径解析和图片托管留给 Phase 2-C CLI 实现）

暂不做：历史、持久化、托盘扩展、CLI、show/hide/hide-all、GET /api/pins。

### Phase 2-B：持久化 + 历史 + 生命周期

必须做：

- 文件系统持久化（`~/.agent-pin/pins/` + `state.json`，见 `02_architecture.md` §5）
- `GET /api/pins`
- `POST /api/pins/{pinId}/show`
- `POST /api/pins/{pinId}/hide`
- `POST /api/pins/hide-all`
- 关闭 Pin 后可恢复（混合方案，见下方）
- 应用重启后历史仍在
- 独立管理界面窗口（`?manager=1`）：完整历史列表 + 搜索 + 删除
- Pin 删除能力（`delete_pin` invoke，销毁窗口 + 删持久化文件 + 更新 state.json，不可恢复）
- 托盘最近 5 个 hidden Pin 快恢菜单（动态刷新）

关闭后恢复入口（混合方案）：

- 托盘右键菜单列最近 5 个 hidden Pin，点击快速重新打开（快恢）。
- 独立管理界面窗口负责完整历史、搜索、删除（完整管理）。
- 托盘只做应用存活 + 快恢入口 + 退出应用；Pin 完整生命周期管理走管理界面。

关闭行为（Phase 2-B 实现口径）：

- 关闭 Pin 窗口 = destroy 窗口 + state 标记 hidden（记录保留，可 show 恢复）
- show 时从 registry 内存读 PinDocument，重新创建窗口（窗口位置不持久化，重新级联）
- 删除 = destroy 窗口 + 删 `pins/{pinId}.json` + 从 registry 移除 + 更新 state.json（不可恢复）

### Phase 2-C：Rust agent-pin CLI

必须做：

- `agent-pin health`
- `agent-pin markdown --title "..." --file ./review.md`
- `agent-pin markdown --title "..." --text "..."`
- `agent-pin image --title "..." --path ./image.png --caption "..."`
- `agent-pin status --title "..." --level success --text "..."`
- `agent-pin push --file ./pin.json`
- `agent-pin list`
- `agent-pin show <pinId>`
- `agent-pin hide <pinId>`
- `agent-pin hide-all`
- `agent-pin --json <command>` 供 Agent 稳定解析输出
- `--file -` 从 stdin 读取 markdown 或 Pin JSON
- CLI 把图片复制到 `~/.agent-pin/images/` 再 POST 副本绝对路径给 HTTP：`image --path` 按当前工作目录解析源图片，`push --file ./pin.json` 按 JSON 文件所在目录解析源图片，`push --file -` 按当前工作目录解析源图片。

CLI 底层调用 `http://127.0.0.1:4317`，不复制业务逻辑。

### Phase 2-D：Skill 文档

必须做：

- 创建 `skills/agent-pin/SKILL.md`
- 提供使用规则和示例
- 强调什么时候应该 pin、什么时候不应该 pin

历史能力属于完整 MVP：Pin 关闭后不能直接消失，必须保留记录并可恢复（Phase 2-B 实现）。

## CLI 技术决策

CLI 直接使用 Rust 实现，不再使用 Node.js MVP CLI。

理由：

- Agent Pin 是本地桌面工具，Rust CLI 更符合长期方向。
- 可以和 Tauri backend 复用类型、schema 和错误码。
- 避免后续从 Node.js CLI 迁移到 Rust CLI。

## MVP 验收标准

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
- 最近 Pin 可以从托盘快恢重新打开
- 管理界面可查看完整历史、搜索、删除
- 应用重启后历史仍在
- 非法 JSON 不崩溃
- 图片路径不存在时显示错误块
- 端口被占用时启动弹窗 + 退出
- 不包含 choice、事件回流、Artifact

## 后续路线

`AGENTS.md` 明确不做 choice、事件回流、MCP、Artifact。以下路线只保留与该约束兼容的项：

- **v0.2**：透明度调节、锁定位置、边缘吸附、Pin 右键菜单（关闭/置顶切换/复制内容）
- **v0.5**：HTTP 增强，更新/删除/查询 Pin 的更多能力
- **v0.7**：手动 Pin、剪贴板 Pin、截图 Pin

被 `AGENTS.md` 排除、不列入路线：choice pin（v0.3）、事件回流（v0.4）、MCP Server（v0.6）。
