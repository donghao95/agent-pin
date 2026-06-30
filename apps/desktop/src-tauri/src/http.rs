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

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
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
                pin_id,
                rollback_err
            );
        }
        return Err(err_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            PinErrorCode::WindowCreateFailed,
            e,
        ));
    }

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

    // 3. 销毁窗口（如果存在）。窗口不存在不算错误。
    if let Err(e) = crate::window::hide_pin_window(&app, &pin_id) {
        eprintln!("[agent-pin] hide_pin_window for {}: {}", pin_id, e);
    }

    // 4. 设状态 hidden
    if let Err(e) = crate::registry::REGISTRY.set_state(&pin_id, PinState::Hidden) {
        return Err(err_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            PinErrorCode::InternalError,
            e,
        ));
    }

    // 5. 刷新托盘菜单（该 Pin 进入 hidden 快恢列表）
    crate::tray::refresh(&app);

    Ok(Json(json!({ "ok": true })))
}

// ---------- POST /api/pins/hide-all ----------

async fn hide_all_pins(State(app): State<AppHandle>) -> Json<Value> {
    let metas = crate::registry::REGISTRY.list();
    for meta in metas {
        if meta.state != PinState::Visible {
            continue;
        }
        // 销毁窗口
        if let Err(e) = crate::window::hide_pin_window(&app, &meta.pin_id) {
            eprintln!("[agent-pin] hide-all window for {}: {}", meta.pin_id, e);
        }
        // 设状态 hidden
        if let Err(e) = crate::registry::REGISTRY.set_state(&meta.pin_id, PinState::Hidden) {
            eprintln!("[agent-pin] hide-all set_state for {}: {}", meta.pin_id, e);
        }
    }
    Json(json!({ "ok": true }))
}

// ---------- 错误响应辅助 ----------

/// 构造统一错误响应。
fn err_response(
    status: StatusCode,
    code: PinErrorCode,
    message: String,
) -> (StatusCode, Json<Value>) {
    let code_value = serde_json::to_value(&code)
        .unwrap_or_else(|_| Value::String("INTERNAL_ERROR".into()));
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

fn show_pin_err_response(message: String) -> (StatusCode, Json<Value>) {
    if message.starts_with("pin not found:") {
        return err_response(StatusCode::NOT_FOUND, PinErrorCode::PinNotFound, message);
    }
    if message.starts_with("failed to create pin window:") {
        return err_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            PinErrorCode::WindowCreateFailed,
            message,
        );
    }
    err_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        PinErrorCode::InternalError,
        message,
    )
}
