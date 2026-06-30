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
INTERNAL_ERROR
```

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
  "pinId": "pin_20260630_121530_pr_review"
}
```

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

Phase 1 只要求支持 `markdown` block。`image`、`status` 和多 block 混排在 Phase 2 补齐。

---

## 5. GET /api/pins

Phase：2

列出最近 Pin。

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
      "pinId": "pin_20260630_121530_pr_review",
      "title": "PR 审查结果",
      "createdAt": "2026-06-30T12:15:30+08:00",
      "visible": true
    }
  ]
}
```

---

## 6. POST /api/pins/{pinId}/show

Phase：2

重新显示一个已隐藏 Pin。

```http
POST /api/pins/{pinId}/show
```

响应：

```json
{
  "ok": true
}
```

---

## 7. POST /api/pins/{pinId}/hide

Phase：2

隐藏一个 Pin。关闭窗口时可以复用这个逻辑。

```http
POST /api/pins/{pinId}/hide
```

响应：

```json
{
  "ok": true
}
```

---

## 8. POST /api/pins/hide-all

Phase：2

隐藏所有当前可见 Pin。

```http
POST /api/pins/hide-all
```

响应：

```json
{
  "ok": true
}
```

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
