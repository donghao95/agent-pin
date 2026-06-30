# AGENTS.md

本文件给参与开发 Agent Pin 的 Codex / Claude / Trae / CodeBuddy 等开发 Agent 使用。

它是本项目的开发宪法。开发 Agent 必须优先遵守本文件，其次再参考 docs、prompts 和具体任务说明。

## 项目目标

Agent Pin 是一个本地桌面工具，让 Agent 可以通过 CLI / HTTP 把重要内容推送成独立桌面 Pin 窗口。

第一版只验证一个核心体验：

> Agent 生成的重要信息，能直接出现在桌面上，而不是淹没在聊天框里。

产品形态接近 PixPin 的“贴图/贴纸”体验，但内容来源是 Agent。

---

## 项目结构

```text
apps/desktop/       # Tauri 桌面应用
packages/cli/       # agent-pin CLI
packages/shared/    # Pin JSON 类型、schema、校验逻辑
docs/               # 规格文档
skills/agent-pin/   # 给外部 Agent 使用的通用 skill
examples/pins/      # Pin JSON 示例
prompts/            # 给实现 Agent 的提示词
```

说明：

- `AGENTS.md`：给开发本项目的 Agent 使用。
- `skills/agent-pin/SKILL.md`：给外部 Agent 使用 Agent Pin 产品的通用 skill。
- 不要把开发约束写进 `skills/agent-pin/SKILL.md`。
- 不要把产品使用 skill 写成某个工具私有目录的唯一版本。

---

## 文档事实来源

遇到需求、实现、审查、修改时，优先查对应事实源，不要凭印象扩展。

- MVP 范围：`docs/mvp-spec.md`
- 架构：`docs/architecture.md`
- HTTP API：`docs/api.md`
- CLI 行为：`docs/cli.md`
- Skill 设计：`docs/skill-design.md`
- 通用 skill：`skills/agent-pin/SKILL.md`
- 示例 Pin：`examples/pins/`
- 实现提示词：`prompts/implement-mvp.md`
- 审查提示词：`prompts/review-mvp.md`

如果文档之间冲突：

1. 先以 `AGENTS.md` 的边界约束为准。
2. 再以 `docs/mvp-spec.md` 的 MVP 范围为准。
3. API、CLI、Skill 的细节分别以对应文档为准。
4. 发现冲突时，应先指出冲突并修正文档，不要直接按自己的理解实现。

---

## 第一性原理分析规则

分析任何问题、需求、设计、缺陷、重构、PR 或实现方案时，必须从第一性原理出发，而不是直接套模板或堆功能。

必须先回答：

1. 用户真正要解决的根本问题是什么？
2. 这个问题背后的不变量是什么？
3. 当前方案破坏了哪个不变量，或者满足了哪个核心约束？
4. 最小可行解是什么？
5. 哪些功能只是“看起来有用”，但会扩大 MVP 范围？

在 Agent Pin 中，最重要的不变量是：

> Agent 可以把重要内容稳定、轻量、低打扰地贴到桌面上。

因此，任何设计都不能为了“功能完整”而破坏：

- 轻量
- 本地优先
- 纯展示 MVP
- 一个 Pin 一个独立窗口
- Agent 通过 CLI / HTTP 稳定调用
- 不做 choice / 事件回流 / Artifact

---

## 问题修复原则

修问题要先找到问题本质和被破坏的不变量，不要用展示兜底、静默吞错、重复状态、临时分支等方式掩盖根因。

修复 Pin 不显示、窗口状态错误、HTTP 成功但窗口未创建、关闭后无法恢复、图片不显示、CLI 调用失败等问题时，必须追踪完整链路：

```text
CLI 参数
  ↓
HTTP 请求
  ↓
PinDocument 校验
  ↓
窗口创建
  ↓
前端渲染
  ↓
state 保存
  ↓
托盘恢复
```

规则：

- 不要只在前端显示层兜底。
- 不要静默吞掉错误。
- 不要让坏输入导致应用崩溃。
- 图片路径不存在、JSON 无效、block 类型不支持等问题必须有明确错误返回或错误展示。
- 如果需要兼容历史坏数据，必须说明原因、范围和验证方式。

---

## MVP 范围

必须做：

- Tauri 2 桌面应用
- 系统托盘
- 本地 HTTP API，监听 `127.0.0.1:4317`
- `agent-pin` CLI
- 一个 Pin 一个独立窗口
- 支持 `markdown` / `image` / `status` blocks
- 一个 Pin 可以包含多个 block
- Pin 窗口支持拖动、缩放、置顶、关闭
- 最近 Pin 简单历史，关闭后可从托盘重新打开
- 基础错误处理，坏输入不能导致应用崩溃

明确不做：

- 不做 choice
- 不做点击事件回流
- 不做 Agent 自动继续执行
- 不做 Artifact 网页
- 不做 table block，表格用 Markdown 表格表达
- 不做云同步、账号、远程访问
- 不做截图、OCR、录屏、图片编辑
- 不做 MCP Server
- 不做 WebSocket 实时通道
- 不做远程 token / 权限系统
- 不做复杂主题系统

如需讨论未来功能，写入 roadmap 或产品 notes，不要混入 MVP 实现事实。

---

## 产品与架构边界

- MVP 是纯展示工具，不是 Agent 平台。
- MVP 只做 Agent → App，不做 User → Agent。
- MVP 不要求 Agent 等待用户反馈。
- MVP 不实现事件队列。
- MVP 不实现选择按钮。
- MVP 不渲染任意 HTML Artifact。
- MVP 不开放局域网访问。
- MVP 不做云端服务。

禁止提前引入：

- `choice` block
- `events` API
- WebSocket
- remote access token
- account / workspace cloud sync
- HTML artifact renderer
- MCP tools

除非用户明确要求调整 MVP 边界，否则不要实现这些功能。

---

## 技术路线

推荐：

- 桌面：Tauri 2
- 前端：React + TypeScript
- 后端：Rust / Tauri backend
- CLI：MVP 可用 Node.js CLI，后续可迁移 Rust CLI
- 本地 API：`http://127.0.0.1:4317`

HTTP 只监听 `127.0.0.1`，不要开放局域网。

---

## 开发顺序

严格按阶段做，不要一次性扩展：

1. Tauri 应用骨架 + 托盘
2. 本地 HTTP 服务：`GET /api/health`、`POST /api/pins`
3. 收到 Markdown Pin 后创建独立窗口
4. 支持 image/status blocks
5. 支持多 block 混排
6. 实现 `agent-pin` CLI
7. 最近 Pin 历史和托盘恢复
8. Skill 文档

每个 PR 尽量只完成一个阶段。

---

## Pin 数据模型规则

- Pin 是窗口。
- Block 是内容。
- 不要设计成 `MarkdownPin` / `ImagePin` 两套对象。
- 一个 Pin 可以混合 Markdown、图片和状态块。
- PinDocument 是 API、CLI、窗口渲染之间的核心契约。

核心结构：

```ts
export type PinDocument = {
  version: 1
  title: string
  blocks: PinBlock[]
  window?: PinWindowConfig
  source?: PinSource
  createdAt?: string
}
```

修改 Pin JSON 结构时，必须同步更新：

- `docs/mvp-spec.md`
- `docs/api.md`
- `docs/cli.md`（如果影响 CLI）
- `skills/agent-pin/SKILL.md`（如果影响 Agent 使用方式）
- `examples/pins/`

---

## 状态与窗口生命周期

MVP 最小 Pin 状态：

- `visible`：窗口当前可见
- `hidden`：窗口已关闭/隐藏，但记录保留
- `failed`：创建失败

窗口生命周期：

```text
created → visible → hidden → visible
created → failed
```

规则：

- 关闭窗口只代表 `hidden`，不代表删除。
- MVP 不做 `deleted` 状态。
- MVP 不做事件回流状态。
- MVP 不做 choice 状态。
- 最近 Pin 历史应基于 Pin 记录，而不是仅基于当前窗口实例。

---

## API 与 CLI 规则

- CLI 是 Agent 的优先入口。
- HTTP API 是桌面应用的底层入口。
- CLI 底层调用 HTTP，不应复制一套独立业务逻辑。
- `POST /api/pins` 必须校验请求体。
- 非法 JSON、空 blocks、图片不存在等情况不能让应用崩溃。
- 图片路径不存在时，在 Pin 内显示错误 block，而不是拒绝整个 Pin。
- Markdown 表格和代码块需要基本可读，窄窗口下可以横向滚动。
- 多个 Pin 创建时要级联排列，避免完全重叠。
- 关闭窗口不等于删除 Pin。

---

## Skill 规则

`skills/agent-pin/SKILL.md` 是给外部 Agent 使用 Agent Pin 的说明，不是开发规范。

Skill 应强调：

- 什么时候应该 pin
- 什么时候不应该 pin
- 优先使用 `agent-pin` CLI
- 如何创建 markdown / image / status / mixed Pin
- MVP 不支持 choice、事件回流、Artifact

修改 CLI 或 Pin JSON 契约时，必须同步更新 skill。

---

## 功能方案文档流转

未落地、正在讨论或包含未来路线判断的功能方案，不要写成 MVP 已实现事实。

规则：

- 未来功能可以写入 `docs/roadmap.md` 或 `docs/product-notes.md`。
- 不要把未来设想写进 `docs/mvp-spec.md` 的已实现范围。
- 功能实现完成并验收通过后，再把稳定事实合并进：
  - `docs/mvp-spec.md`
  - `docs/api.md`
  - `docs/cli.md`
  - `docs/architecture.md`
  - `skills/agent-pin/SKILL.md`
- 合并时只迁移当前实现事实，不要复制过程性讨论或未确认计划。

---

## 对抗式审查规则

在做任何提交、创建 PR、合并 PR、请求用户验收之前，必须进行对抗式审查。

要求：

- 在支持子代理的环境中，必须使用独立子代理进行 adversarial review。
- 子代理应站在反方角度审查：这个改动是否破坏 MVP 边界、是否引入过度设计、是否遗漏错误处理、是否与文档契约冲突。
- 对抗式审查必须覆盖：需求理解、架构边界、API/CLI 契约、Pin 状态、错误处理、测试与验收。
- 审查发现的问题必须先处理或明确记录为后续问题，再提交/开 PR。
- 不要把普通自我总结当作对抗式审查。

如果当前工具环境不支持子代理：

- 必须明确说明“无法使用子代理”的原因。
- 必须执行一次等价的反方清单审查。
- 除非用户明确豁免，否则不要跳过提交前审查。

推荐审查问题：

1. 这个改动是否违背“纯展示 MVP”？
2. 是否偷偷加入 choice、events、Artifact、MCP、云同步等非 MVP 功能？
3. 是否破坏 PinDocument 契约？
4. CLI、HTTP、前端渲染是否仍使用同一事实源？
5. 坏输入是否会导致崩溃或静默失败？
6. 关闭窗口、恢复窗口、最近历史是否语义一致？
7. 文档、示例、skill 是否同步更新？
8. 是否有最小可验证命令或步骤？

---

## 本地开发命令

当前仓库尚未初始化代码。实现后应补充实际命令。

预期命令形态：

```bash
pnpm install
pnpm dev
pnpm build
pnpm lint
pnpm typecheck
pnpm test
```

第一阶段最重要的验收命令：

```bash
curl -X POST http://127.0.0.1:4317/api/pins \
  -H "Content-Type: application/json" \
  -d '{"version":1,"title":"测试 Pin","blocks":[{"type":"markdown","content":"## Hello Agent Pin\n这是第一个 Pin。"}]}'
```

桌面应出现一个独立 Markdown Pin 窗口。

---

## 测试与验收要求

窄范围改动先跑最小相关检查；触及共享契约、API、CLI 或窗口生命周期时，要扩大验证范围。

必须验证：

- `GET /api/health`
- `POST /api/pins`
- Markdown Pin 创建
- Image Pin 创建
- Status Pin 创建
- mixed Pin 创建
- 非法 JSON
- 空 blocks
- 不支持 block type
- 图片路径不存在
- 多 Pin 级联排列
- 关闭后恢复

如果有测试无法运行，需要说明原因。

---

## Windows Shell 约定

本项目优先保证 Windows 可运行。

- PowerShell 使用 `Get-Content` 读文件时，加 `-Encoding UTF8`。
- 搜索文本或文件优先用 `rg`。
- 不要默认使用 Bash-only 脚本。
- 路径处理必须兼容 Windows 反斜杠、空格路径和 Unix 风格路径。
- 图片路径示例应尽量覆盖 Windows 路径和相对路径。

---

## PR / Commit 约束

- 每个 PR 尽量只做一个阶段。
- 不要在 MVP 阶段引入 choice、事件回流、MCP、Artifact。
- 不要提前做复杂美化、主题系统或云同步。
- 优先保证最小闭环可运行。
- 提交前必须进行对抗式审查。
- PR 描述必须包含：改了什么、没做什么、如何验证、是否更新文档。

---

## 最小验收标准

第一阶段最小闭环：

1. 应用启动。
2. HTTP 服务监听 `127.0.0.1:4317`。
3. `GET /api/health` 返回 ok。
4. `POST /api/pins` 接收 Markdown Pin JSON。
5. 桌面出现一个独立 Markdown Pin 窗口。
6. Pin 可拖动、缩放、置顶、关闭。
7. 非法请求不导致应用崩溃。
