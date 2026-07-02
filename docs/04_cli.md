# agent-pin CLI 设计

CLI 名称：

```text
agent-pin
```

CLI 是给 Agent / Skill 使用的稳定入口，底层调用本地 HTTP API：

```text
http://127.0.0.1:4317
```

Agent 应优先使用 CLI，而不是直接写 curl。机器调用建议始终加 `--json`，便于稳定解析 `pinId` 和错误码。

## 1. 设计原则

CLI 直接使用 Rust 实现，不再使用 Node.js MVP CLI。

CLI 不包含复杂业务逻辑，只负责：

1. 读取参数、文件或 stdin。
2. 组装 Pin JSON。
3. 将 image 路径解析为本地文件，并复制到 `~/.agent-pin/images/`。
4. 调用本地 HTTP API。
5. 输出人类可读文本或机器可解析 JSON。

Agent-facing 约束：

- 默认 endpoint 只允许本地回环地址，避免把 Pin 内容发送到远程主机。
- `--json` 下成功和失败都输出固定 JSON 结构。
- `--file -` 表示从 stdin 读取。
- `markdown --file/--text` 二选一，必须提供且只能提供一个。
- CLI 创建 image/mixed Pin 时不会长期引用源图片路径，而是把图片复制到 `~/.agent-pin/images/` 后提交副本路径；源文件移动后不影响 Pin 重新打开。
- CLI 只提前做能提升反馈质量的轻量校验，最终契约校验仍以 desktop/shared 校验为准。

---

## 2. 全局参数

```bash
agent-pin --json <command>
agent-pin --endpoint http://127.0.0.1:4318 <command>
```

### --json

成功输出示例：

```json
{"ok":true,"pinId":"pin_1782801843675_717272"}
```

失败输出示例：

```json
{"ok":false,"error":{"code":"INVALID_PIN_DOCUMENT","message":"invalid pin document: ..."}}
```

`--json` 不改变退出码：成功为 `0`，失败为非 `0`。

### --endpoint

默认 endpoint：

```text
http://127.0.0.1:4317
```

可以通过 `--endpoint` 参数或 `AGENT_PIN_ENDPOINT` 环境变量覆盖：

```bash
agent-pin --endpoint http://127.0.0.1:4318 health
AGENT_PIN_ENDPOINT=http://127.0.0.1:4318 agent-pin health
```

endpoint 校验规则（防 SSRF）：

- 仅允许 `http` scheme。
- 仅允许 host 为 `127.0.0.1`、`localhost` 或 `[::1]`。
- 拒绝 userinfo。
- 拒绝 path、query、fragment。
- 端口号不限，支持多实例调试。

---

## 3. agent-pin health

检查桌面应用是否运行。

```bash
agent-pin health
agent-pin --json health
```

人类输出：

```text
Agent Pin 正在运行。
版本: 0.1.0
Endpoint: http://127.0.0.1:4317
```

JSON 输出：

```json
{"ok":true,"running":true,"version":"0.1.0","endpoint":"http://127.0.0.1:4317"}
```

未运行时退出码为非 `0`。

---

## 4. agent-pin markdown

创建 Markdown Pin。适合结论、摘要、审查结果、短报告。

从文件读取：

```bash
agent-pin --json markdown --title "PR 审查结果" --file ./review.md
```

从 stdin 读取：

```powershell
Get-Content -Encoding UTF8 .\review.md | agent-pin --json markdown --title "PR 审查结果" --file -
```

从命令行文本读取：

```bash
agent-pin --json markdown --title "结论" --text "第一版应该做成 Tauri 桌面 Pin。"
```

`--file` 和 `--text` 二选一（互斥），必须提供其中一个。

可选参数：

```bash
--width 420
--height 360
--height auto
--no-always-on-top
--agent codex
--workspace TryCue
--task "PR Review"
```

`--height` 行为：

- `"auto"`（默认）：窗口初始高度 200px，前端渲染后测量内容高度自适应调整，上限为屏幕高度的 70%。
- 数值（如 `360`）：直接使用指定高度，仍受屏幕高度 70% 上限约束。

---

## 5. agent-pin image

创建 Image Pin。适合展示图片结果或关键截图。

```bash
agent-pin --json image --title "装修效果图" --path ./render.png
agent-pin --json image --title "装修效果图" --path ./render.png --caption "入门柜参考图"
```

`--path` 可以是相对路径。CLI 会基于当前工作目录找到图片，把它复制到 `~/.agent-pin/images/`，再把副本绝对路径 POST 给 desktop。支持扩展名：PNG/JPG/JPEG/WebP/GIF。

CLI 要求源图片存在且是文件；源图片不存在时直接返回 `IMAGE_NOT_FOUND`，不会创建 Pin。

同样支持 `--width` / `--height` / `--no-always-on-top` / `--agent` / `--workspace` / `--task`。

---

## 6. agent-pin status

创建 Status Pin。适合任务状态、成功、警告、失败。

```bash
agent-pin --json status --title "TryCue 审查状态" --level success --text "审查完成：发现 2 个问题。"
```

`level` 可选：

```text
info
success
warning
error
```

如果省略 `--level`，desktop 端按 `info` 展示。

同样支持 `--width` / `--height` / `--no-always-on-top` / `--agent` / `--workspace` / `--task`。

---

## 7. agent-pin push

推送完整 Pin JSON。Agent 自动创建 Pin 时默认使用该命令；它可以覆盖 Markdown、图片、状态和混合内容。

```bash
agent-pin --json push --file ./pin.json
```

从 stdin 读取：

```powershell
Get-Content -Encoding UTF8 .\pin.json | agent-pin --json push --file -
```

示例 `pin.json`：

```json
{
  "version": 1,
  "title": "装修效果参考",
  "blocks": [
    {
      "type": "markdown",
      "content": "## 说明\n这张图适合作为入门柜风格参考。"
    },
    {
      "type": "image",
      "path": "./cabinet.png",
      "caption": "入门柜效果图"
    }
  ]
}
```

图片使用建议：

- 使用 `push --file ./pin.json` 时，建议把图片放在 `pin.json` 旁边，或在 JSON 中写清楚相对位置。
- 使用 `push --file -` 时，确保命令从包含图片的工作目录运行；不确定时用绝对路径。
- 创建成功后，Agent Pin 会保存图片副本；源图片移动或删除不影响已创建的 Pin。
- 源图片不存在时返回 `IMAGE_NOT_FOUND`，不会创建 Pin。此时检查路径、工作目录或改用绝对路径。

预期成功输出：

```json
{"ok":true,"pinId":"pin_1782801843675_717272"}
```

桌面应出现一个新的 Pin。没有出现时先运行 `agent-pin --json health`，再检查命令返回的 `error.code` 和 `error.message`。

---

## 8. agent-pin list

列出最近 Pin。

```bash
agent-pin list
agent-pin --json list
```

人类输出示例：

```text
pin_1782801843675_717272  PR 审查结果  visible
pin_1782801900100_283945  装修效果图  hidden
```

JSON 输出直接返回 HTTP `GET /api/pins` 的响应结构。

---

## 9. agent-pin show

重新显示一个已隐藏 Pin。

```bash
agent-pin --json show pin_1782801900100_283945
```

JSON 输出：

```json
{"ok":true,"pinId":"pin_1782801900100_283945","state":"visible"}
```

---

## 10. agent-pin hide

隐藏一个当前可见 Pin。记录保留，可重新 `show`。

```bash
agent-pin --json hide pin_1782801900100_283945
```

JSON 输出：

```json
{"ok":true,"pinId":"pin_1782801900100_283945","state":"hidden"}
```

---

## 11. agent-pin hide-all

隐藏全部当前可见 Pin。

```bash
agent-pin --json hide-all
```

JSON 输出：

```json
{"ok":true}
```

---

## 12. Agent 使用原则

Agent 使用 CLI 时应尽量：

- 默认加 `--json`。
- 先用 `agent-pin --json health` 判断桌面应用是否运行。
- 保持 Pin 内容简短。
- 不把完整聊天记录 pin 出来。
- 优先 pin 结论、风险、状态、图片结果。
- 自动创建 Pin 时优先用 `push --file`；`markdown`、`image`、`status` 作为用户明确要求或手写调试时的快捷命令。
- 图片路径错误时先检查工作目录，必要时改用绝对路径。
