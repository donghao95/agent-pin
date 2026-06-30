# Agent Pin Skill 设计

本文档说明 Agent Pin 的 skill 应该如何设计。

## 1. Skill 目标

当 Agent 需要把重要内容贴到用户桌面时，调用 `agent-pin` CLI 创建 Pin。

这个 skill 不负责开发 Agent Pin 本身，而是告诉外部 Agent 如何使用 Agent Pin。

## 2. Skill 目录

通用 skill 放在：

```text
skills/agent-pin/SKILL.md
```

这是产品的一部分，可以复制到不同 Agent 工具中。

如果后续要适配具体工具，可以再复制到：

```text
.claude/skills/agent-pin/SKILL.md
.codebuddy/skills/agent-pin/SKILL.md
```

但仓库内的 canonical 版本应保留在 `skills/agent-pin/SKILL.md`。

## 3. 什么时候应该 pin

Agent 应该在这些情况下 pin：

- 用户明确说“pin 到桌面”“贴到桌面”“生成一个 pin”
- 长任务完成
- 发现高风险问题
- 生成了用户需要稍后看的结论
- 生成了图片结果
- 总结出关键决策
- 信息不应该淹没在聊天框里

## 4. 什么时候不应该 pin

Agent 不应该在这些情况下 pin：

- 普通聊天回复
- 中间推理
- 低价值解释
- 重复信息
- 未确认猜测
- 用户只是随口问答
- 内容太长且没有整理
- 私密信息未经用户要求不应外显到桌面

## 5. 支持内容

MVP 支持：

- Markdown Pin
- Image Pin
- Status Pin
- 混合 Pin，也就是一个 Pin 包含多个 blocks

MVP 不支持：

- choice
- 用户点击事件
- 事件回流
- 完整 HTML Artifact
- 远程分享

## 6. 使用优先级

Agent 使用 Agent Pin 时：

1. 优先使用 `agent-pin` CLI。
2. 如果 CLI 不可用，再使用 HTTP API。
3. 如果 HTTP 不可用，最后才使用文件夹投递协议。

MVP skill 中重点写 CLI，不鼓励 Agent 直接拼 curl。

## 7. 内容质量规则

Agent 生成 Pin 时应遵守：

- 一个 Pin 只表达一个明确目的。
- 标题要短。
- 内容要比聊天回复更精炼。
- 不要把完整聊天记录贴出去。
- Markdown 表格可以使用，但不要过宽。
- 图片应配简短说明。
- Status 应一眼能看出状态级别。

## 8. 推荐命令

Markdown：

```bash
agent-pin markdown --title "PR 审查结果" --file ./review.md
```

Image：

```bash
agent-pin image --title "装修效果图" --path ./render.png --caption "入门柜参考图"
```

Status：

```bash
agent-pin status --title "任务完成" --level success --text "审查完成：发现 2 个问题。"
```

Mixed：

```bash
agent-pin push --file ./pin.json
```

## 9. 后续扩展

后续版本可以扩展：

- choice pin
- `agent-pin events`
- MCP tools
- 手动 Pin
- 剪贴板 Pin

但这些都不属于 MVP。
