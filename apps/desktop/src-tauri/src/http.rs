// 本地 HTTP 服务
//
// 监听 127.0.0.1:4317，只暴露给本机，不开放局域网。
// 路由：
//   GET  /api/health  -> 健康检查
//   POST /api/pins    -> 创建 Pin 窗口
//
// 请求体大小限制：1MB（Markdown 足够，Phase 2 图片走本地路径不走 HTTP body）。
// 错误处理：坏输入返回统一错误响应，不 panic。
//
// 契约来源：docs/api.md

use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde_json::{json, Value};
use tauri::AppHandle;
use tower_http::limit::RequestBodyLimitLayer;

/// 请求体最大 1MB
const MAX_BODY: usize = 1024 * 1024;

/// 启动 HTTP server。在 Tauri setup hook 中通过 tauri::async_runtime::spawn 调用。
pub async fn start_http(app: AppHandle) {
    let router = Router::new()
        .route("/api/health", get(health))
        .route("/api/pins", post(create_pin))
        .layer(RequestBodyLimitLayer::new(MAX_BODY))
        .with_state(app);

    let listener = match tokio::net::TcpListener::bind("127.0.0.1:4317").await {
        Ok(l) => l,
        Err(e) => {
            eprintln!(
                "[agent-pin] failed to bind 127.0.0.1:4317: {}. \
                 请确认 Agent Pin 未重复启动，且端口未被占用。",
                e
            );
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
                crate::pin::PinErrorCode::InvalidJson,
                format!("invalid JSON: {}", e),
            ))
        }
    };

    // 2. 校验 PinDocument
    if let Err(e) = crate::pin::validate(&doc) {
        return Err(err_response(StatusCode::BAD_REQUEST, e.code, e.message));
    }

    // 3. 生成 pinId 并存入 registry（窗口渲染时通过 invoke 读取）
    //    clone 一份给窗口创建用，原 doc 存入 registry
    let pin_id = crate::registry::generate_pin_id();
    crate::registry::REGISTRY.insert(pin_id.clone(), doc.clone());

    // 4. 创建窗口
    if let Err(e) = crate::window::create_pin_window(&app, &pin_id, &doc) {
        // 窗口创建失败：回滚 registry
        crate::registry::REGISTRY.remove(&pin_id);
        return Err(err_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            crate::pin::PinErrorCode::WindowCreateFailed,
            e,
        ));
    }

    Ok(Json(json!({ "ok": true, "pinId": pin_id })))
}

/// 构造统一错误响应。
fn err_response(
    status: StatusCode,
    code: crate::pin::PinErrorCode,
    message: String,
) -> (StatusCode, Json<Value>) {
    let code_value = serde_json::to_value(&code).unwrap_or_else(|_| Value::String("INTERNAL_ERROR".into()));
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
