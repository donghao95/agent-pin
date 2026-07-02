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
2. 如何用 `push` 构造 Pin JSON。
3. 如何遵守用户个人 pinning 偏好。

## 2. 目录结构

```text
skills/agent-pin/
  SKILL.md
  references/
    preference-initialization.md
```

设计理由：

- `SKILL.md` 保持短，只放核心流程和命令。
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
3. 如果偏好文件无法读取、格式明显损坏或内容互相冲突，先告诉用户问题；用户同意后备份旧文件并重建。
4. 偏好文件写好后，再按用户偏好创建 Pin，或判断当前内容是否值得 pin。
5. 如果偏好文件没有覆盖当前场景，先根据当前用户消息判断；仍不确定时，问用户是否要 pin，或问是否要把这类场景写入偏好文件。

偏好文件不存在是阻塞步骤：Agent 不应继续检查 CLI、安装、启动或创建 Pin；应先按 `preference-initialization.md` 一次只问一个问题完成初始化。

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

## 7. 创建方式

Agent 自动创建 Pin 时默认使用 `push --file`，因为完整 Pin JSON 可以覆盖 Markdown、图片、状态和混合内容。

- 单段 Markdown：用 `push` 创建一个 `markdown` block。
- 单张图片：用 `push` 创建一个 `image` block。图片创建成功后不依赖源文件原路径。
- 短状态：用 `push` 创建一个 `status` block。
- 混合内容：用 `push` 创建多个 blocks。
- `markdown` / `image` / `status` 子命令只作为用户明确要求或手写调试时的快捷方式保留。
- `list` / `show` / `hide` / `hide-all`：只在用户请求或继续既有 pin workflow 时使用。

Agent 调用 CLI 时默认使用 `--json`。

CLI 失败时不要改走 HTTP。`health` 失败只在聊天里提示用户启动桌面应用；图片不存在、不可读或格式不支持时，只报告错误并要求用户提供可用图片。任务本身失败是否 pin，按偏好文件和失败策略判断。

## 8. 安装与启动

完成偏好检查后，Skill 应指导 Agent 运行 `agent-pin --json health`。如果 CLI 缺失，先征得用户同意，再从 GitHub Releases 下载 CLI zip。下载 Release、运行下载程序、启动本地程序、修改 PATH 都会改变用户机器状态，必须先得到用户同意。

如果桌面应用未运行，先提示用户启动 Agent Pin；在用户同意后，Agent 可尝试启动本地已安装的 `Agent Pin.exe`。

Skill 不应指导终端用户安装 Rust 工具链或 Node 构建环境。源码构建路径属于开发者文档，不写入 SKILL.md。

## 9. MVP 边界

MVP 支持：Markdown Pin、Image Pin、Status Pin、混合 Pin、Pin 历史、show/hide/hide-all。

MVP 不支持：choice、用户点击事件、事件回流、完整 HTML Artifact、MCP Server、远程分享、文件夹投递协议。

Skill 不应暗示这些能力存在。

## 10. Skill 验证

修改 skill 后必须至少运行 `skill-creator` 的 `quick_validate.py`。

当 skill 行为涉及策略判断、偏好初始化、是否 pin 等容易误用的流程时，应按 `skill-creator` 的 forward-testing 建议，使用独立子代理做验证。子代理提示应像真实用户任务，而不是泄露预期答案；验证重点包括：

- 用户明确要求 pin 但没有偏好文件时，是否先分步询问并写入 `~/.agent-pin/pin-preferences.md`，再调用 CLI。
- 用户给出持久纠正时，是否更新偏好文件；一次性选择是否避免误写成长期规则。
- Agent 自动创建 Pin 时是否默认使用 `push --file`。
- 图片 Pin 是否通过 CLI 托管副本，而不是长期依赖源文件路径。
- CLI 缺失、桌面未运行、需要下载安装或启动程序时，是否先征得用户同意。
- 是否避免 choice、事件回流、Artifact、MCP 等 MVP 外能力。
