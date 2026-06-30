# 实现 Agent Pin MVP 的提示词

把下面提示词交给 Codex / Claude / Trae / CodeBuddy，用于第一轮实现。

```text
请根据本仓库文档实现 Agent Pin MVP。

目标：做一个 Tauri 2 桌面应用，让 Agent 可以通过 agent-pin CLI 或本地 HTTP API，把 Markdown、图片、状态信息推送成独立桌面 Pin 窗口。

第一版只做纯展示，不做 choice、不做用户点击事件回流、不做 Artifact、不做云同步、不做账号系统。

请严格阅读：
- AGENTS.md
- docs/mvp-spec.md
- docs/api.md
- docs/cli.md
- docs/architecture.md
- skills/agent-pin/SKILL.md

第一轮只实现最小可运行闭环：
1. Tauri 2 应用可启动。
2. 应用启动后监听 127.0.0.1:4317。
3. 实现 GET /api/health。
4. 实现 POST /api/pins。
5. POST /api/pins 接收 JSON：
   {
     "version": 1,
     "title": "测试 Pin",
     "blocks": [
       {
         "type": "markdown",
         "content": "## Hello\n这是一个测试 Pin。"
       }
     ]
   }
6. 每次收到合法请求，都创建一个独立 Pin 窗口。
7. Pin 窗口显示 title 和 markdown 内容。
8. Pin 窗口默认置顶、可拖动、可缩放、可关闭。
9. 暂时不做 image、status、CLI、历史、choice、事件回流、Artifact。

实现要求：
- 优先保证 Windows 可运行。
- 不要扩展功能。
- 不要提前做复杂主题、云同步、MCP、choice。
- 坏输入不能让应用崩溃。
- 提交后说明如何运行和如何用 curl 创建测试 Pin。

验收 curl：

curl -X POST http://127.0.0.1:4317/api/pins \
  -H "Content-Type: application/json" \
  -d '{"version":1,"title":"测试 Pin","blocks":[{"type":"markdown","content":"## Hello Agent Pin\n这是第一个 Pin。"}]}'

预期结果：桌面出现一个独立 Markdown Pin 窗口。
```

## 第二轮实现提示词

```text
在第一轮 HTTP + Markdown Pin 闭环已经跑通的基础上，继续实现：

1. image block。
2. status block。
3. 一个 Pin 多个 block 混排。
4. 基础 Markdown 样式，包括代码块和 Markdown 表格。
5. 图片路径不存在时，在 Pin 内显示错误块，不要崩溃。
6. 多个 Pin 级联排列，避免完全重叠。

仍然不要做 choice、事件回流、Artifact、MCP、云同步。
```

## 第三轮实现提示词

```text
在桌面应用和 blocks 渲染完成后，实现 agent-pin CLI。

命令：
- agent-pin health
- agent-pin markdown --title "..." --file ./review.md
- agent-pin markdown --title "..." --text "..."
- agent-pin image --title "..." --path ./image.png --caption "..."
- agent-pin status --title "..." --level success --text "..."
- agent-pin push --file ./pin.json

CLI 底层调用 http://127.0.0.1:4317。
```
