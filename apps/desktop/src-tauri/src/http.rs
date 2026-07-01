// 本地 HTTP 服务
//
// 监听 127.0.0.1:4317，只暴露给本机，不开放局域网。
// 路由：
//   GET  /api/health             -> 健康检查
//   POST /api/pins               -> 创建 Pin 窗口
//   GET  /api/pins               -> 列出所有 Pin 元数据
//   POST /api/pins/{pinId}/show  -> 重新显示已隐藏 Pin
//   POST /api/pins/{pinId}/hide  -> 隐藏 Pin（destroy 窗口 + state=hidden）
//   POST /api/pins/hide-all      -> 隐藏所有可见 Pin
//
// 请求体大小限制：1MB（Markdown 足够，图片走本地路径不走 HTTP body）。
// 错误处理：坏输入返回统一错误响应，不 panic。
//
// 契约来源：docs/api.md
// 路径参数使用 axum 0.8+ 的 {pinId} 语法（不是 :pinId）。

use axum::extract::Request;
use axum::{
    extract::{Path, State},
    http::{Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use serde_json::{json, Value};
use tauri::AppHandle;
use tower_http::limit::RequestBodyLimitLayer;

use crate::pin::PinErrorCode;
use crate::storage::PinState;

/// 请求体最大 1MB
const MAX_BODY: usize = 1024 * 1024;

/// Pin 总数上限。防止本地恶意进程循环创建海量 Pin 导致窗口句柄/GDI 耗尽、
/// 内存膨胀、磁盘膨胀、state.json 全量重写阻塞 I/O（M2 DoS 防护）。
const MAX_PIN_COUNT: usize = 500;

/// 启动 HTTP server。listener 由 setup hook 同步绑定后传入。
/// bind 在 setup hook 中执行，失败可直接弹窗 + 退出；这里只负责把 std listener
/// 转 tokio listener 并 serve。避免在 async task 中 bind 失败后无法通知主线程退出。
pub async fn start_http(app: AppHandle, std_listener: std::net::TcpListener) {
    let router = Router::new()
        .route("/api/health", get(health))
        .route("/api/pins", post(create_pin).get(list_pins))
        .route("/api/pins/hide-all", post(hide_all_pins))
        .route("/api/pins/{pinId}/show", post(show_pin))
        .route("/api/pins/{pinId}/hide", post(hide_pin))
        .layer(RequestBodyLimitLayer::new(MAX_BODY))
        // CSRF 防护：POST 请求必须带 Content-Type: application/json。
        // 浏览器对 application/json 的跨站 POST 会发 preflight（OPTIONS），
        // 我们不响应 CORS，preflight 失败 → 实际请求不会发出。
        // text/plain 是简单请求不发 preflight，必须拒绝。
        // CLI 的 ureq 已显式设 Content-Type: application/json，不受影响。
        .layer(middleware::from_fn(csrf_guard))
        .with_state(app);

    // std listener -> tokio listener（非阻塞已在 setup hook 中设置）
    let listener = match tokio::net::TcpListener::from_std(std_listener) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[agent-pin] failed to convert std listener to tokio: {}", e);
            return;
        }
    };

    eprintln!("[agent-pin] HTTP server listening on http://127.0.0.1:4317");
    if let Err(e) = axum::serve(listener, router).await {
        eprintln!("[agent-pin] http server error: {}", e);
    }
}

async fn health() -> Json<Value> {
    Json(json!({
        "ok": true,
        "app": "Agent Pin",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

// ---------- POST /api/pins ----------

async fn create_pin(
    State(app): State<AppHandle>,
    body: String,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // 1. 解析 JSON
    let doc: crate::pin::PinDocument = match serde_json::from_str(&body) {
        Ok(d) => d,
        Err(e) => {
            return Err(err_response(
                StatusCode::BAD_REQUEST,
                PinErrorCode::InvalidJson,
                format!("invalid JSON: {}", e),
            ))
        }
    };

    // 2. 校验 PinDocument
    if let Err(e) = crate::pin::validate(&doc) {
        return Err(err_response(StatusCode::BAD_REQUEST, e.code, e.message));
    }

    // M2：Pin 总数限制，防 DoS。超过上限返回 409 Conflict。
    if crate::registry::REGISTRY.list().len() >= MAX_PIN_COUNT {
        return Err(err_response(
            StatusCode::CONFLICT,
            PinErrorCode::InternalError,
            format!("pin count limit reached (max {})", MAX_PIN_COUNT),
        ));
    }

    // 3. 生成 pinId 并持久化（写 pins/{pinId}.json + state.json）
    //    insert 返回 Result<PinMeta, String>：doc 写盘失败时返回 Err
    let pin_id = crate::registry::generate_pin_id();
    if let Err(e) = crate::registry::REGISTRY.insert(pin_id.clone(), doc.clone()) {
        return Err(err_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            PinErrorCode::InternalError,
            format!("failed to persist pin: {}", e),
        ));
    }

    // 4. 创建窗口
    if let Err(e) = crate::window::create_pin_window(&app, &pin_id, &doc) {
        // 窗口创建失败：回滚 registry（删 pins/{pinId}.json + 更新 state.json）
        if let Err(rollback_err) = crate::registry::REGISTRY.remove(&pin_id) {
            eprintln!(
                "[agent-pin] rollback remove failed for {}: {}",
                pin_id, rollback_err
            );
        }
        return Err(err_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            PinErrorCode::WindowCreateFailed,
            e,
        ));
    }

    // 5. pins:changed 事件由 registry::insert 内部 emit，管理界面和托盘自动刷新

    Ok(Json(json!({ "ok": true, "pinId": pin_id })))
}

// ---------- GET /api/pins ----------

async fn list_pins() -> Json<Value> {
    let pins = crate::registry::REGISTRY.list();
    // PinMeta 已是 camelCase Serialize，直接转 Value
    let pins_value = serde_json::to_value(&pins).unwrap_or_else(|_| Value::Array(vec![]));
    Json(json!({
        "ok": true,
        "pins": pins_value,
    }))
}

// ---------- POST /api/pins/{pinId}/show ----------

async fn show_pin(
    State(app): State<AppHandle>,
    Path(pin_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if let Err(e) =
        crate::pin_actions::show_pin(&app, &pin_id, crate::pin_actions::ShowPinMode::Sync)
    {
        return Err(show_pin_err_response(e));
    }

    Ok(Json(json!({ "ok": true })))
}

// ---------- POST /api/pins/{pinId}/hide ----------

async fn hide_pin(
    State(app): State<AppHandle>,
    Path(pin_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // 1. 检查 pin 存在
    let meta = match crate::registry::REGISTRY.get_meta(&pin_id) {
        Some(m) => m,
        None => {
            return Err(err_response(
                StatusCode::NOT_FOUND,
                PinErrorCode::PinNotFound,
                format!("pin not found: {}", pin_id),
            ))
        }
    };

    // 2. 幂等：已 hidden 直接返回 ok
    if meta.state == PinState::Hidden {
        return Ok(Json(json!({ "ok": true })));
    }

    // 3. 销毁窗口（如果存在）。
    //    M2 修复：窗口销毁失败不再静默吞掉。若窗口存在但 destroy 失败，
    //    窗口仍可见，此时设 state=hidden 会导致内存与实际不一致。
    //    返回 500 让调用方知道窗口未被关闭，可重试。
    //    窗口不存在（幂等 Ok）不触发此分支。
    if let Err(e) = crate::window::hide_pin_window(&app, &pin_id) {
        return Err(err_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            PinErrorCode::InternalError,
            format!("failed to destroy window for {}: {}", pin_id, e),
        ));
    }

    // 4. 设状态 hidden
    if let Err(e) = crate::registry::REGISTRY.set_state(&pin_id, PinState::Hidden) {
        return Err(err_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            PinErrorCode::InternalError,
            e,
        ));
    }

    // 5. tray 刷新由 set_state(Hidden) 触发 registry emit "pins:changed" → tray listen 自动处理

    Ok(Json(json!({ "ok": true })))
}

// ---------- POST /api/pins/hide-all ----------

/// m4：与 Tauri hide_all_pins 命令行为统一——有失败时返回非 2xx，
/// 让调用方明确知道操作未完全成功。HTTP 返回 207 Multi-Status + failed 列表。
async fn hide_all_pins(
    State(app): State<AppHandle>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let failed = crate::pin_actions::hide_all_visible(&app);
    if failed.is_empty() {
        Ok(Json(json!({ "ok": true })))
    } else {
        Err(err_response(
            StatusCode::MULTI_STATUS,
            PinErrorCode::InternalError,
            format!("some pins could not be hidden: {}", failed.join(", ")),
        ))
    }
}

// ---------- 错误响应辅助 ----------

/// 构造统一错误响应。
fn err_response(
    status: StatusCode,
    code: PinErrorCode,
    message: String,
) -> (StatusCode, Json<Value>) {
    let code_value =
        serde_json::to_value(&code).unwrap_or_else(|_| Value::String("INTERNAL_ERROR".into()));
    (
        status,
        Json(json!({
            "ok": false,
            "error": {
                "code": code_value,
                "message": message,
            }
        })),
    )
}

/// M1 修复：用类型化 ShowPinError 替代字符串前缀匹配，可靠映射 HTTP 状态码。
fn show_pin_err_response(e: crate::pin_actions::ShowPinError) -> (StatusCode, Json<Value>) {
    use crate::pin_actions::ShowPinError;
    let (status, code, message) = match e {
        ShowPinError::NotFound(msg) => (StatusCode::NOT_FOUND, PinErrorCode::PinNotFound, msg),
        ShowPinError::Failed(msg) => (StatusCode::CONFLICT, PinErrorCode::InternalError, msg),
        ShowPinError::WindowCreate(msg) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            PinErrorCode::WindowCreateFailed,
            msg,
        ),
        ShowPinError::Internal(msg) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            PinErrorCode::InternalError,
            msg,
        ),
    };
    err_response(status, code, message)
}

// ---------- CSRF 防护 ----------

/// POST 请求必须带 Content-Type: application/json，否则拒绝。
/// 浏览器对 application/json 的跨站 POST 会发 preflight（OPTIONS），
/// 我们不响应 CORS，preflight 失败 → 实际请求不会发出。
/// text/plain 是简单请求不发 preflight，必须拒绝（防 CSRF）。
/// CLI 的 ureq 已显式设 Content-Type: application/json，不受影响。
///
/// m1 防御纵深：额外校验 Host 头，只允许 127.0.0.1:4317 和 localhost:4317。
/// 防 DNS rebinding 攻击（攻击者把恶意域名 DNS 解析到 127.0.0.1）。
async fn csrf_guard(req: Request, next: Next) -> Response {
    if req.method() == Method::POST {
        // m1：Host 头白名单校验（防御纵深，防 DNS rebinding）
        let host = req
            .headers()
            .get(axum::http::header::HOST)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if host != "127.0.0.1:4317" && host != "localhost:4317" {
            return err_response(
                StatusCode::FORBIDDEN,
                PinErrorCode::InternalError,
                "host not allowed".to_string(),
            )
            .into_response();
        }

        let ct = req
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if !ct.starts_with("application/json") {
            return err_response(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                PinErrorCode::InvalidJson,
                "Content-Type must be application/json".to_string(),
            )
            .into_response();
        }
    }
    next.run(req).await
}
