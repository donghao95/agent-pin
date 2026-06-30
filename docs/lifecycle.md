# Agent Pin 生命周期说明

## 1. 核心原则

托盘或未来管理界面表示 Agent Pin 应用是否仍在运行。

Pin 窗口本身只是内容窗口，不负责表示整个应用是否存活。

## 2. Phase 1 生命周期

Phase 1 只验证最小闭环：HTTP 请求可以创建 Markdown Pin 窗口。

Phase 1 可以有一个最小应用存活入口，例如托盘或一个简单管理窗口。

推荐口径：

- Pin 窗口关闭：关闭该 Pin 窗口。
- 关闭 Pin 不等于退出 Agent Pin 应用。
- Agent Pin 应用仍然可以继续监听 `127.0.0.1:4317`。
- 退出 Agent Pin 应用：关闭所有 Pin 窗口，并停止 HTTP 服务。
- Phase 1 不承诺关闭 Pin 后还能恢复。
- Phase 1 不做磁盘历史。
- Phase 1 不做最近 Pin 列表。

因此，Phase 1 的“关闭不等于删除”只作为产品原则保留，不要求实现跨关闭恢复。

## 3. 托盘在 Phase 1 的角色

如果 Phase 1 做托盘，托盘只负责应用级生命周期。

最小菜单：

- Quit Agent Pin

可选菜单：

- About / Agent Pin is running

Phase 1 托盘不负责：

- 恢复单个 Pin。
- 展示历史列表。
- 显示最近 Pin。
- 删除 Pin。
- 管理 Pin。

## 4. 未来管理界面

Phase 2 或后续版本可以引入管理界面。

管理界面负责：

- 查看最近 Pin。
- 重新打开已关闭 Pin。
- 删除 Pin 记录。
- 搜索 Pin。
- 管理设置。
- 查看 Agent Pin 是否运行。

长期更合理的形态是：

```text
托盘：表示应用活着，提供退出和打开管理界面
管理界面：管理历史、恢复、删除、搜索、设置
Pin 窗口：只负责展示一条具体内容
```

## 5. Phase 2 生命周期

Phase 2 开始实现历史能力后，关闭 Pin 的语义变为：

- 关闭 Pin：隐藏或关闭窗口，但保留 Pin 记录。
- 管理界面或托盘入口可以重新打开 Pin。
- 删除 Pin：未来需要明确的删除动作，不等同于关闭窗口。

Phase 2 可以实现：

- `~/.agent-pin/pins/`
- `~/.agent-pin/state.json`
- 最近 Pin 列表
- 重新打开已关闭 Pin
- `GET /api/pins`
- `POST /api/pins/:pinId/show`
- `POST /api/pins/:pinId/hide`

## 6. 不要混淆的概念

- 关闭 Pin 窗口，不等于退出应用。
- 退出应用，应该关闭所有 Pin 并停止 HTTP 服务。
- 关闭 Pin，在 Phase 1 不保证恢复。
- 关闭 Pin，在 Phase 2 应保留历史记录。
- 托盘不应该承担复杂 Pin 管理。
- 管理界面才是未来的 Pin 历史和设置中心。
