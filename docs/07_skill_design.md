# Agent Pin Skill 设计

本文档说明 Agent Pin 的 canonical skill 应该如何设计。实际 skill 位于：

```text
skills/agent-pin/SKILL.md
```

这个 skill 给外部 Agent 使用 Agent Pin，不负责开发 Agent Pin 本身。

## 1. 核心目标

当 Agent 需要把重要内容贴到用户桌面时，优先调用 `agent-pin --json` CLI 创建 Pin。

Skill 的核心不是“教会所有命令”，而是帮助 Agent 做三个判断：

1. 是否应该 pin。
2. 应该用 `status` / `markdown` / `image` / `push` 哪个命令。
3. 如何遵守用户个人 pinning 偏好。

## 2. 目录结构

```text
skills/agent-pin/
  SKILL.md
  references/
    default-pinning-policy.md
    preference-initialization.md
```

设计理由：

- `SKILL.md` 保持短，只放核心流程和命令。
- `references/default-pinning-policy.md` 放默认策略。
- `references/preference-initialization.md` 放首次偏好初始化访谈流程。
- 用户个人偏好不放进 skill 包，而是放在 `~/.agent-pin/pin-preferences.md`。
- Skill 正文和 references 面向中文用户，默认使用中文说明；命令名和 JSON 字段保持英文原文。

个人偏好不放进 skill 包的原因：

- canonical skill 可发布、复制和升级。
- 用户偏好是本机状态，不应提交到仓库。
- 仓库更新不应覆盖个人习惯。

## 3. 偏好文件

用户偏好文件路径：

```text
~/.agent-pin/pin-preferences.md
```

Agent 使用 skill 时：

1. 如果偏好文件存在，先读偏好文件。
2. 如果不存在，读取 `references/preference-initialization.md`，分步询问并生成偏好文件。
3. 偏好文件写好后，再按用户偏好创建 Pin，或判断当前内容是否值得 pin。
4. 如果偏好文件存在但没有覆盖当前场景，读取 `references/default-pinning-policy.md` 作为默认策略。

偏好文件只能在以下情况修改：

- 偏好文件不存在，需要首次初始化。
- 用户明确要求更新 pinning 偏好。
- 用户给出明确、持久的纠正，例如“以后不要 pin 测试失败”。

不要从一次性拒绝或普通聊天里静默学习偏好；只有持续习惯、规则或纠正才写入偏好文件。

## 4. 什么时候应该 pin

默认策略偏保守。应该 pin：

- 用户明确说“pin 到桌面”“贴到桌面”“生成一个 pin”。
- 长任务完成且结果需要留在桌面。
- 发现高风险问题或阻塞问题。
- 生成了图片或视觉结果。
- 总结出关键决策、结论或下一步。
- 用户说“稍后看”“提醒我”“放桌面”。

## 5. 什么时候不应该 pin

不应该 pin：

- 普通聊天回复。
- 中间推理。
- 未确认猜测。
- 重复低价值状态。
- 完整聊天记录。
- 未整理的长日志。
- secrets、tokens、credentials、隐私或敏感业务数据，除非用户明确要求。
- Agent Pin 自己不可用这类工具故障。

## 6. 失败是否 pin

失败 pin 的价值：让“需要稍后处理的阻塞结果”留在桌面。

失败 pin 的代价：临时错误会打扰用户，且可能把敏感错误信息外显到桌面。

默认规则：

- 任务级失败、CI/build/test 失败、高风险审查问题：可以 pin。
- Agent Pin 未启动、CLI 参数错误、临时网络错误、权限未授权：只在聊天里报告。
- 包含密钥、路径隐私、客户数据的错误：不要 pin，除非用户明确要求且内容已脱敏。

## 7. 命令选择

- `status`：短状态、成功、警告、失败。
- `markdown`：结论、摘要、审查结果、决策、下一步。
- `image`：单张本地图片。CLI 会复制图片到 `~/.agent-pin/images/`，不要长期引用源文件路径。
- `push`：混合内容，例如 status + markdown、markdown + image。混合 Pin 中的图片同样由 CLI 复制到 `~/.agent-pin/images/`。
- `list` / `show` / `hide` / `hide-all`：只在用户请求或继续既有 pin workflow 时使用。

Agent 调用 CLI 时默认使用 `--json`。

## 8. MVP 边界

MVP 支持：

- Markdown Pin
- Image Pin
- Status Pin
- 混合 Pin
- Pin 历史、show/hide/hide-all

MVP 不支持：

- choice
- 用户点击事件
- 事件回流
- 完整 HTML Artifact
- MCP Server
- 远程分享
- 文件夹投递协议

Skill 不应暗示这些能力存在。

## 9. Skill 验证

修改 skill 后必须至少运行 `skill-creator` 的 `quick_validate.py`。

当 skill 行为涉及策略判断、偏好初始化、是否 pin 等容易误用的流程时，应按 `skill-creator` 的 forward-testing 建议，使用独立子代理做验证。子代理提示应像真实用户任务，而不是泄露预期答案；验证重点包括：

- 用户明确要求 pin 但没有偏好文件时，是否先分步询问并写入 `~/.agent-pin/pin-preferences.md`，再调用 CLI。
- 用户给出持久纠正时，是否更新偏好文件；一次性选择是否避免误写成长期规则。
- 图片 Pin 是否通过 CLI 托管副本，而不是长期依赖源文件路径。
- 是否避免 choice、事件回流、Artifact、MCP 等 MVP 外能力。
