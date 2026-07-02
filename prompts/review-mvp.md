# 审查 Agent Pin MVP 的提示词

```text
请审查当前 Agent Pin MVP 实现。

重点检查它是否严格符合 MVP 范围，而不是扩展功能。

必须核对：
1. 是否实现 Tauri 桌面应用。
2. 是否监听 127.0.0.1:4317。
3. GET /api/health 是否可用。
4. POST /api/pins 是否能创建独立 Pin 窗口。
5. Pin 是否支持 markdown / image / status blocks。
6. 一个 Pin 是否可以包含多个 block。
7. 窗口是否可拖动、缩放、置顶、关闭。
8. 是否实现 agent-pin CLI。
9. CLI 是否通过 HTTP 调用桌面应用。
10. 坏输入是否不会导致应用崩溃。
11. 图片路径不存在时是否有可读错误。
12. 是否没有引入 choice、事件回流、Artifact、MCP、云同步、账号系统。

请输出：
- 必须修复的问题
- 可以后续再做的问题
- 是否满足 MVP 验收
```
