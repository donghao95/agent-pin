# agent-pin CLI 设计

CLI 名称：

```text
agent-pin
```

CLI 是给 Agent / Skill 使用的稳定入口。CLI 底层调用本地 HTTP API：

```text
http://127.0.0.1:4317
```

Agent 应优先使用 CLI，而不是直接写 curl。

## 1. agent-pin health

检查桌面应用是否运行。

```bash
agent-pin health
```

成功输出：

```text
Agent Pin is running.
Version: 0.1.0
Endpoint: http://127.0.0.1:4317
```

失败输出：

```text
Agent Pin is not running.
Please start the desktop app first.
```

## 2. agent-pin markdown

创建 Markdown Pin。

从文件读取：

```bash
agent-pin markdown --title "PR 审查结果" --file ./review.md
```

从命令行文本读取：

```bash
agent-pin markdown --title "结论" --text "第一版应该做成 Tauri 桌面 Pin。"
```

可选参数：

```bash
--width 420
--height 360
--no-always-on-top
--agent codex
--workspace TryCue
--task "PR Review"
```

## 3. agent-pin image

创建 Image Pin。

```bash
agent-pin image --title "装修效果图" --path ./render.png
```

带说明：

```bash
agent-pin image --title "装修效果图" --path ./render.png --caption "入门柜参考图"
```

## 4. agent-pin status

创建 Status Pin。

```bash
agent-pin status --title "TryCue 审查状态" --level success --text "审查完成：发现 2 个问题。"
```

`level` 可选：

```text
info
success
warning
error
```

## 5. agent-pin push

推送完整 Pin JSON。用于混合内容，例如 Markdown + Image。

```bash
agent-pin push --file ./pin.json
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
      "path": "C:/Users/hao/Desktop/cabinet.png",
      "caption": "入门柜效果图"
    }
  ]
}
```

## 6. agent-pin list

列出最近 Pin。

```bash
agent-pin list
```

示例输出：

```text
pin_20260630_121530_pr_review  PR 审查结果  visible
pin_20260630_122100_cabinet    装修效果图  hidden
```

## 7. agent-pin show

重新显示一个已隐藏 Pin。

```bash
agent-pin show pin_20260630_122100_cabinet
```

## 8. agent-pin hide-all

隐藏全部当前可见 Pin。

```bash
agent-pin hide-all
```

## 9. Endpoint 配置

默认 endpoint：

```text
http://127.0.0.1:4317
```

可以通过环境变量覆盖：

```bash
AGENT_PIN_ENDPOINT=http://127.0.0.1:4318 agent-pin health
```

## 10. CLI 实现建议

MVP 可以使用 Node.js 实现 CLI，后续再考虑 Rust 单文件二进制。

原因：

- 处理 Markdown 文件方便
- 处理 JSON 方便
- Agent 调试容易
- 开发速度快

CLI 不应该包含复杂业务逻辑，只负责：

1. 读取参数和文件。
2. 组装 Pin JSON。
3. 调用本地 HTTP API。
4. 输出明确结果。

## 11. Agent 使用原则

Agent 使用 CLI 时应尽量：

- 保持 Pin 内容简短
- 不把完整聊天记录 pin 出来
- 优先 pin 结论、风险、状态、图片结果
- 混合内容使用 `agent-pin push --file`
- 不创建 choice，因为 MVP 暂不支持交互
