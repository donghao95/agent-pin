---
name: agent-pin
description: 当用户要求把内容 pin/贴到桌面、创建桌面便签/状态/图片，初始化或更新 Agent Pin 偏好，或重要 Agent 输出适合通过本地 agent-pin CLI 显示为桌面 Pin 时使用。支持 markdown、image、status、mixed pins，以及用户 pin 偏好初始化流程。
---

# Agent Pin

使用 `agent-pin` 把重要 Agent 输出发送到用户桌面，显示为轻量本地 Pin 窗口。

只创建 `markdown`、`image`、`status` 或混合 blocks 的展示型 Pin。不要创建 choice pin、按钮、事件回流、HTML Artifact 或 MCP；Pin 是纯展示，不等待用户交互。

## 第一步

创建 Pin 或自主判断是否应该 pin 之前，先检查用户偏好文件：

```text
~/.agent-pin/pin-preferences.md
```

如果文件存在，先读取并遵守，除非当前用户消息明确覆盖它。

如果文件不存在，读取 `references/preference-initialization.md`，用多步问题引导用户说出偏好，并写入 `~/.agent-pin/pin-preferences.md`。偏好文件写好后，再继续创建 Pin 或判断当前内容是否值得 pin。

偏好文件不存在时，不要继续检查 `agent-pin --json health`、安装 CLI、启动桌面应用或创建 Pin。先完成偏好初始化；初始化问题一次只问一个。

如果偏好文件无法读取、格式明显损坏或内容互相冲突，先在聊天里说明问题，询问用户是否重建偏好文件；不要猜测用户习惯。

如果偏好文件没有覆盖当前场景，先根据当前用户消息判断；仍不确定时，问一个简短问题确认是否要 pin，或问是否要把这类场景写入偏好文件。

当用户给出持久偏好或纠正，例如“以后不要 pin 测试失败”“以后长任务完成都 pin 一下”，更新 `~/.agent-pin/pin-preferences.md` 的学习记录和对应规则。不要把一次性选择自动固化为长期偏好。

## 安装与启动

完成偏好检查后，再检查 CLI 和桌面应用。只有偏好文件已经存在，或用户当前请求只是安装/启动 Agent Pin 时，才进入本段流程。

```bash
agent-pin --json health
```

预期成功时返回 `ok: true`，桌面应用已在本机运行，可以创建 Pin。

如果 `agent-pin` 命令不存在：

- 先告诉用户需要安装 Agent Pin CLI，并询问是否允许下载 Release 资产；不要静默下载或执行安装包。
- 用户同意后，从 `https://github.com/donghao95/agent-pin/releases` 下载当前 Release 的 `agent-pin-*-windows-x64.zip`，解压出 `agent-pin.exe`，放到用户同意的位置并加入当前会话 PATH，或让用户把它加入系统 PATH。

如果 CLI 存在但 health 失败：

- 先提示用户启动 Agent Pin 桌面应用。
- 启动本地已安装程序前先询问用户是否允许。不要在未授权时调用 `Start-Process`。
- 启动后重新运行 `agent-pin --json health`。仍失败时，只在聊天里报告，不要创建“Agent Pin 不可用”的 Pin。

下载 Release、运行下载的 EXE、启动本地程序、修改 PATH 都会改变用户机器状态，执行前必须得到用户同意。

## 默认创建方式

优先使用 CLI，Agent 调用默认加 `--json`：

```bash
agent-pin --json health
```

如果 health 失败，不要创建“Agent Pin 不可用”的 Pin；直接告诉用户先启动桌面应用。

其他 CLI 命令失败时，直接在聊天里报告结构化错误和下一步，不要改走 HTTP，也不要为了报告工具故障再创建错误 Pin。任务本身的失败是否 pin，按用户偏好和失败策略判断。

默认用 `push` 创建 Pin，因为它覆盖 Markdown、图片、状态和混合内容：

```bash
agent-pin --json push --file ./pin.json
```

也可以从 stdin 传入：

```powershell
Get-Content -Encoding UTF8 .\pin.json | agent-pin --json push --file -
```

预期成功时返回：

```json
{"ok":true,"pinId":"pin_..."}
```

桌面应出现一个新的 Pin。Pin 关闭后会保留在历史中，可用 `list` 和 `show` 恢复。

最小 Markdown Pin：

```json
{
  "version": 1,
  "title": "审查结果",
  "blocks": [
    {
      "type": "markdown",
      "content": "## 结论\n通过。"
    }
  ]
}
```

状态 + Markdown：

```json
{
  "version": 1,
  "title": "构建失败",
  "blocks": [
    {
      "type": "status",
      "level": "error",
      "text": "测试失败，需要处理。"
    },
    {
      "type": "markdown",
      "content": "## 下一步\n1. 查看失败用例。\n2. 修复后重新运行测试。"
    }
  ]
}
```

Markdown + 图片：

```json
{
  "version": 1,
  "title": "效果图",
  "blocks": [
    {
      "type": "markdown",
      "content": "## 说明\n这是本次生成的视觉结果。"
    },
    {
      "type": "image",
      "path": "./render.png",
      "caption": "生成图"
    }
  ]
}
```

图片路径可以写相对路径。使用 `push --file ./pin.json` 时，把图片放在 `pin.json` 旁边或写清楚相对位置；使用 `--file -` 时，确保命令从包含图片的工作目录运行。创建成功后，Agent Pin 会保存图片副本，源文件之后移动也不影响已创建的 Pin。

短命令 `markdown`、`image`、`status` 仍可在用户明确要求或手写调试时使用；Agent 自动创建 Pin 时优先用 `push`。

## 问题排查

- `command not found`：CLI 未安装或不在 PATH。按“安装与启动”处理。
- `health` 失败：桌面应用未运行、端口被占用，或 endpoint 被改过。先启动桌面应用，再重试。
- `IMAGE_NOT_FOUND`：图片路径不对、文件被移动，或从 stdin 调用时工作目录不对。改用绝对路径，或把图片放到 `pin.json` 旁边再重试。
- `IMAGE_UNSUPPORTED`：改用 png、jpg、jpeg、webp 或 gif。
- `INVALID_PIN_DOCUMENT`：检查 JSON 是否有效、`version` 是否为 `1`、`title` 是否非空、`blocks` 是否至少有一个。
- 命令返回 `ok:false` 时，把 `error.code` 和 `error.message` 简短报告给用户，并给出下一步。

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
- 不要假设存在事件回流、HTML Artifact、MCP 或远程分享。
