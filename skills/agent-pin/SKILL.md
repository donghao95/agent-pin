---
name: agent-pin
description: 当用户要求把内容 pin/贴到桌面、创建桌面便签/状态/图片，初始化或更新 Agent Pin 偏好，或重要 Agent 输出适合通过本地 agent-pin CLI 显示为桌面 Pin 时使用。支持 markdown、image、status、mixed pins，以及用户 pin 偏好初始化流程。
---

# Agent Pin

使用 `agent-pin` 把重要 Agent 输出发送到用户桌面，显示为轻量本地 Pin 窗口。

MVP 只做展示。不要假设存在按钮、用户反馈事件、choice pin、HTML Artifact、MCP 或远程分享。

## 第一步

创建 Pin 或自主判断是否应该 pin 之前，先检查用户偏好文件：

```text
~/.agent-pin/pin-preferences.md
```

如果文件存在，先读取并遵守，除非当前用户消息明确覆盖它。

如果文件不存在，读取 `references/preference-initialization.md`，用多步问题引导用户说出偏好，并写入 `~/.agent-pin/pin-preferences.md`。偏好文件写好后，再继续创建 Pin 或判断当前内容是否值得 pin。

如果偏好文件存在但没有覆盖当前场景，读取 `references/default-pinning-policy.md` 作为默认策略。

当用户给出持久偏好或纠正，例如“以后不要 pin 测试失败”“以后长任务完成都 pin 一下”，更新 `~/.agent-pin/pin-preferences.md` 的学习记录和对应规则。不要把一次性选择自动固化为长期偏好。

## 命令规则

优先使用 CLI，Agent 调用默认加 `--json`：

```bash
agent-pin --json health
```

如果 health 失败，不要创建“Agent Pin 不可用”的 Pin；直接告诉用户先启动桌面应用。

按内容类型选择命令：

- `status`：短状态、成功、警告、失败。
- `markdown`：结论、审查结果、决策、下一步摘要。
- `image`：单张本地图片，可带 caption。
- `push`：混合内容，尤其是 status + markdown 或 markdown + image。
- `list`、`show`、`hide`、`hide-all`：只在用户请求或继续既有 Pin workflow 时使用。

## 创建 Pin

Markdown 文件：

```bash
agent-pin --json markdown --title "审查结果" --file ./review.md
```

Markdown stdin：

```powershell
Get-Content -Encoding UTF8 .\review.md | agent-pin --json markdown --title "审查结果" --file -
```

短 Markdown 文本：

```bash
agent-pin --json markdown --title "结论" --text "MVP 应继续聚焦本地桌面 Pin。"
```

图片：

```bash
agent-pin --json image --title "效果图" --path ./image.png --caption "参考图"
```

通过 CLI 创建图片 Pin 时，CLI 会把图片复制到 `~/.agent-pin/images/` 后再创建 Pin。Pin 创建成功后，不依赖原始图片继续留在原路径。

状态：

```bash
agent-pin --json status --title "任务完成" --level success --text "审查完成，发现 2 个问题。"
```

允许的 status level：

```text
info
success
warning
error
```

混合 Pin：

```bash
agent-pin --json push --file ./pin.json
```

`push` 中的相对图片路径按 JSON 文件所在目录解析；使用 `--file -` 时按当前工作目录解析。找到图片后，CLI 会复制到 `~/.agent-pin/images/` 并在 PinDocument 中写入副本路径。

混合 JSON 示例：

```json
{
  "version": 1,
  "title": "审查摘要",
  "blocks": [
    {
      "type": "status",
      "level": "warning",
      "text": "发现 2 个问题，建议先修第一个。"
    },
    {
      "type": "markdown",
      "content": "## 下一步\n1. 修复 shared exports。\n2. 重新运行 typecheck。"
    }
  ]
}
```

## 管理 Pin

Pin 会跨重启保留。关闭 Pin 只会变成 `hidden`，不会删除。

```bash
agent-pin --json list
agent-pin --json show pin_1782801843675_717272
agent-pin --json hide pin_1782801843675_717272
agent-pin --json hide-all
```

## 内容规则

- Pin 应简短、明确、有持续价值。
- 不要 pin 完整聊天记录。
- 不要 pin 内部推理。
- 不要 pin 未整理的原始日志。
- 不要 pin secrets、credentials、个人隐私或敏感业务数据，除非用户明确要求且内容已脱敏。
- 一个 Pin 只表达一个目的。
- 优先先写结论，再写最少证据或下一步。
- 不要创建 choice pin，也不要等待用户通过 Pin 交互。

MVP 没有文件夹投递协议。
