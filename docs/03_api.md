# Agent Pin API 设计

默认本地服务地址：

```text
http://127.0.0.1:4317
```

只监听本地回环地址，不开放局域网。

## 1. 分期口径

### Phase 1 API

Phase 1 只实现最小闭环：

- `GET /api/health`
- `POST /api/pins`

目标是验证：HTTP 请求可以创建一个独立 Markdown Pin 窗口。

### Phase 2 API

Phase 2 再补完整 MVP 的历史和窗口生命周期能力：

- `GET /api/pins`
- `POST /api/pins/{pinId}/show`
- `POST /api/pins/{pinId}/hide`
- `POST /api/pins/hide-all`

历史能力属于完整 MVP，但不阻塞 Phase 1。

注：路径参数使用 `{pinId}` 语法（axum 0.8+）。Phase 1 路由无路径参数，不受影响。

---

## 2. 通用响应

成功：

```json
{
  "ok": true
}
```

失败：

```json
{
  "ok": false,
  "error": {
    "code": "ERROR_CODE",
    "message": "Human readable message"
  }
}
```

常见错误码：

```text
INVALID_JSON
INVALID_PIN_DOCUMENT
UNSUPPORTED_BLOCK_TYPE
IMAGE_NOT_FOUND
IMAGE_UNSUPPORTED
WINDOW_CREATE_FAILED
PIN_NOT_FOUND
INTERNAL_ERROR
```

`PIN_NOT_FOUND` 在 Phase 2-B 引入：`show` / `hide` 路由的 `{pinId}` 在 registry 中不存在时返回 404。`delete`（管理界面 invoke）同样使用此错误码。

`INVALID_JSON` 也用于 POST 请求未携带 `Content-Type: application/json` 的场景（返回 415 UNSUPPORTED_MEDIA_TYPE）。这是 CSRF 防护的一部分：浏览器对非 `application/json` 的 POST 视为简单请求不发 preflight，强制 Content-Type 可阻止跨站 CSRF。

---

## 3. GET /api/health

Phase：1

用于 CLI 和调试检查桌面应用是否启动。

请求：

```http
GET /api/health
```

响应：

```json
{
  "ok": true,
  "app": "Agent Pin",
  "version": "0.1.0"
}
```

---

## 4. POST /api/pins

Phase：1

创建一个新的桌面 Pin 窗口。

请求：

```http
POST /api/pins
Content-Type: application/json
```

**Content-Type 强制要求**：所有 POST 请求必须携带 `Content-Type: application/json`（charset 可选），否则返回 415 + `INVALID_JSON`。这是 CSRF 防护的一部分，阻止浏览器跨站简单 POST。

请求体大小上限：1 MB。

请求体：

```json
{
  "version": 1,
  "title": "PR 审查结果",
  "blocks": [
    {
      "type": "markdown",
      "content": "## 结论\n发现 2 个问题，建议先修第一个。"
    }
  ],
  "window": {
    "width": 420,
    "height": "auto",
    "alwaysOnTop": true
  },
  "source": {
    "agent": "codex",
    "workspace": "TryCue"
  }
}
```

成功响应：

```json
{
  "ok": true,
  "pinId": "pin_1782801843675_717272"
}
```

`pinId` 格式：`pin_<timestamp_ms>_<6位随机数字>`，例如 `pin_1782801843675_717272`。timestamp_ms 是 Unix 毫秒时间戳，6 位随机用于同一毫秒内的冲突避免。

Phase 2-B 起，创建成功后会持久化到 `~/.agent-pin/pins/{pinId}.json` 并在 `state.json` 中记录元数据。窗口创建失败时会回滚（删除已写入的持久化文件）。

失败响应：

```json
{
  "ok": false,
  "error": {
    "code": "INVALID_PIN_DOCUMENT",
    "message": "blocks must contain at least one block"
  }
}
```

Phase 1 只要求支持 `markdown` block。`image`、`status` 和多 block 混排在 Phase 2-A 补齐。

---

## 5. GET /api/pins

Phase：2-B

列出所有 Pin 元数据（按 createdAt 降序）。

请求：

```http
GET /api/pins
```

响应：

```json
{
  "ok": true,
  "pins": [
    {
      "pinId": "pin_1782801843675_717272",
      "title": "PR 审查结果",
      "createdAt": "2026-06-30T12:15:30+08:00",
      "updatedAt": "2026-06-30T12:20:00+08:00",
      "state": "visible",
      "source": {
        "agent": "codex",
        "workspace": "TryCue"
      }
    }
  ]
}
```

字段说明：

- `pinId`：Pin 唯一标识
- `title`：Pin 标题
- `createdAt` / `updatedAt`：ISO 8601 时间戳
- `state`：`visible` / `hidden` / `failed`
  - `visible`：窗口当前可见
  - `hidden`：窗口已关闭/隐藏，但记录保留，可 show 恢复
  - `failed`：Pin 文件损坏或丢失（启动时 load_from_disk 检测到 doc 缺失/损坏会标记为 failed）
- `source`：可选，Pin 来源信息（agent / workspace / task / conversationId）

---

## 6. POST /api/pins/{pinId}/show

Phase：2-B

重新显示一个已隐藏 Pin。从 registry 读取 PinDocument，重新创建窗口。

幂等：如果 Pin 已是 visible 状态，直接返回 ok，不重复创建窗口。

```http
POST /api/pins/{pinId}/show
```

成功响应：

```json
{
  "ok": true
}
```

失败响应（pinId 不存在）：

```json
{
  "ok": false,
  "error": {
    "code": "PIN_NOT_FOUND",
    "message": "pin not found: pin_xxx"
  }
}
```

窗口创建失败返回 `WINDOW_CREATE_FAILED`（500）。

---

## 7. POST /api/pins/{pinId}/hide

Phase：2-B

隐藏一个 Pin：销毁窗口 + state 标记 hidden。记录保留，可从托盘或管理界面恢复。

幂等：如果 Pin 已是 hidden 状态，直接返回 ok。

```http
POST /api/pins/{pinId}/hide
```

成功响应：

```json
{
  "ok": true
}
```

失败响应（pinId 不存在）：

```json
{
  "ok": false,
  "error": {
    "code": "PIN_NOT_FOUND",
    "message": "pin not found: pin_xxx"
  }
}
```

注意：用户点 Pin 窗口关闭按钮会触发 `WindowEvent::Destroyed`，lib.rs 的事件处理器会自动把状态标记为 hidden（如果当前是 visible）。这与 `POST /hide` 走同一套状态机，但路径不同：前者是窗口事件回调，后者是 HTTP 调用。

---

## 8. POST /api/pins/hide-all

Phase：2-B

隐藏所有当前可见 Pin。遍历 registry 中所有 visible 的 Pin，逐个销毁窗口并标记 hidden。

```http
POST /api/pins/hide-all
```

响应：

```json
{
  "ok": true
}
```

即使部分窗口销毁失败，也继续处理其余 Pin，错误记录到 stderr 但不中断。最终始终返回 ok。

---

## 9. 校验规则

`POST /api/pins` 必须校验：

- `version` 必须为 `1`
- `title` 必须存在且非空
- `blocks` 必须存在且至少一个 block
- block type 必须是 `markdown`、`image` 或 `status`（未知 type 在反序列化阶段被拒绝，返回 `INVALID_JSON`）
- markdown block 的 `content` 必须非空
- image block 的 `path` 必须非空且为绝对路径（相对路径解析由 Phase 2-C CLI 处理）
- image block 的 `path` 扩展名必须是 PNG/JPG/JPEG/WebP/GIF 之一（否则 `IMAGE_UNSUPPORTED`）
- image block 的文件存在性不校验：desktop 不知道 Agent cwd，前端 `<img>` onerror 显示错误块
- status block 的 `text` 必须非空
- status block 的 `level` 若存在，必须是 `info`/`success`/`warning`/`error` 之一

---

## 10. 安全边界

MVP 仅本地使用：

- 只监听 `127.0.0.1`
- 不做公网/局域网访问
- 不做账号和权限
- 不做鉴权 token

后续如果开放局域网访问，必须重新设计鉴权和权限模型。
