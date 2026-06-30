# Agent Pin API 设计

MVP 默认本地服务地址：

```text
http://127.0.0.1:4317
```

只监听本地回环地址，不开放局域网。

## 1. 通用响应

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

## 2. GET /api/health

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

## 3. POST /api/pins

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

## 4. GET /api/pins

列出最近 Pin。MVP 可简化实现。

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

## 5. POST /api/pins/:pinId/show

重新显示一个已隐藏 Pin。

```http
POST /api/pins/:pinId/show
```

响应：

```json
{
  "ok": true
}
```

## 6. POST /api/pins/:pinId/hide

隐藏一个 Pin。关闭窗口时可以复用这个逻辑。

```http
POST /api/pins/:pinId/hide
```

响应：

```json
{
  "ok": true
}
```

## 7. POST /api/pins/hide-all

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

## 8. 校验规则

`POST /api/pins` 必须校验：

- `version` 必须为 `1`
- `title` 必须存在且非空
- `blocks` 必须存在且至少一个 block
- block type 必须是 `markdown`、`image` 或 `status`
- markdown block 的 `content` 必须非空
- image block 的 `path` 必须存在；若图片不存在，建议在 Pin 内显示错误 block，不要让应用崩溃
- status block 的 `text` 必须非空

## 9. 安全边界

MVP 仅本地使用：

- 只监听 `127.0.0.1`
- 不做公网/局域网访问
- 不做账号和权限
- 不做鉴权 token

后续如果开放局域网访问，必须重新设计鉴权和权限模型。
