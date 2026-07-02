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
// 契约来源：docs/03_api.md
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
use std::path::Path as FsPath;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::AppHandle;
use tower_http::limit::RequestBodyLimitLayer;

use crate::pin::{PinBlock, PinErrorCode};
use crate::storage::PinState;

/// 请求体最大 1MB
const MAX_BODY: usize = 1024 * 1024;

/// 复制图片到数据目录并改写 doc 中的路径。
/// 如果文件存在且扩展名合法，复制到 ~/.agent-pin/images/ 并改写路径。
/// 如果文件不存在，保留原路径不动（前端 onerror 显示错误块）。
/// 扩展名合法性已由 PinDocument validate() 在调用前统一校验。
/// 如果文件存在但无法托管，返回错误，避免 API 成功但 Pin 必然无法加载图片。
fn copy_image_blocks_to_store(doc: &mut crate::pin::PinDocument) -> Result<(), String> {
    let images_dir = crate::storage::images_dir();
    copy_image_blocks_to_store_in(doc, &images_dir)
}

fn copy_image_blocks_to_store_in(
    doc: &mut crate::pin::PinDocument,
    images_dir: &FsPath,
) -> Result<(), String> {
    let mut changed = false;
    for block in doc.blocks.iter_mut() {
        if let PinBlock::Image(img) = block {
            let src = FsPath::new(&img.path);
            // 只处理绝对路径
            if !src.is_absolute() {
                continue;
            }
            // 校验扩展名
            let ext = match src.extension().and_then(|e| e.to_str()) {
                Some(e) => e.to_ascii_lowercase(),
                None => continue,
            };
            if !crate::pin::IMAGE_EXTENSIONS.contains(&ext.as_str()) {
                continue;
            }
            // 检查文件是否存在
            if !src.exists() {
                continue;
            }
            // 检查是否已在 images 目录内（避免重复复制）
            if let (Ok(canonical_src), Ok(canonical_store)) = (
                std::fs::canonicalize(src),
                std::fs::canonicalize(images_dir),
            ) {
                if canonical_src.starts_with(&canonical_store) {
                    continue;
                }
            }
            // 懒创建目录：只有文件存在且确实需要托管时，目录创建失败才应阻塞 POST。
            if let Err(e) = std::fs::create_dir_all(images_dir) {
                return Err(format!("failed to create images dir: {}", e));
            }
            // 生成目标路径并复制
            let millis = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            let counter = IMAGE_COPY_COUNTER.fetch_add(1, Ordering::Relaxed);
            let dest = images_dir.join(format!(
                "image_{}_{}_{}.{}",
                millis,
                std::process::id(),
                counter,
                ext
            ));
            match std::fs::copy(src, &dest) {
                Ok(_) => {
                    if let Some(dest_str) = dest.to_str() {
                        img.path = dest_str.to_string();
                        changed = true;
                    } else {
                        let _ = std::fs::remove_file(&dest);
                        return Err(format!(
                            "copied image destination path is not valid UTF-8: {}",
                            dest.display()
                        ));
                    }
                }
                Err(e) => {
                    return Err(format!(
                        "failed to copy image '{}' to '{}': {}",
                        img.path,
                        dest.display(),
                        e
                    ));
                }
            }
        }
    }

    if changed {
        crate::pin::validate(doc).map_err(|e| e.message)?;
    }
    Ok(())
}

static IMAGE_COPY_COUNTER: AtomicU64 = AtomicU64::new(0);

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

    // 2.5. 图片托管：复制 image block 的源图片到 ~/.agent-pin/images/ 并改写路径。
    //      文件不存在时保留原路径（前端 onerror 显示错误块）。
    let mut doc = doc;
    if let Err(e) = copy_image_blocks_to_store(&mut doc) {
        return Err(err_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            PinErrorCode::InternalError,
            e,
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

// ---------- Host 白名单 + CSRF 防护 ----------

/// 校验 Host 头是否为本地回环地址（任意端口）。
/// 允许：127.0.0.1:<port>、localhost:<port>、[::1]:<port>。
/// 端口号不限，支持 CLI --endpoint 自定义本地端口调试。
/// 防 DNS rebinding 攻击（攻击者把恶意域名 DNS 解析到 127.0.0.1）。
fn is_localhost_host(host: &str) -> bool {
    if host.is_empty() {
        return false;
    }

    let (hostname, port_part) = if let Some(stripped) = host.strip_prefix('[') {
        let Some(end) = stripped.find(']') else {
            return false;
        };
        let hostname = &stripped[..end];
        let rest = &stripped[end + 1..];
        if rest.is_empty() {
            (hostname, None)
        } else if let Some(port) = rest.strip_prefix(':') {
            (hostname, Some(port))
        } else {
            return false;
        }
    } else {
        if host == "::1" {
            ("::1", None)
        } else {
            if host.contains('[') || host.contains(']') {
                return false;
            }
            match host.rsplit_once(':') {
                Some((hostname, port)) => {
                    if hostname.contains(':') {
                        return false;
                    }
                    (hostname, Some(port))
                }
                None => (host, None),
            }
        }
    };

    if let Some(port) = port_part {
        if port.is_empty() || port.parse::<u16>().is_err() {
            return false;
        }
    }

    hostname.eq_ignore_ascii_case("localhost") || hostname == "127.0.0.1" || hostname == "::1"
}

/// 所有 /api 请求都校验 Host 头（防 DNS rebinding）。
/// POST 请求额外校验 Content-Type: application/json（防 CSRF）。
///
/// 浏览器对 application/json 的跨站 POST 会发 preflight（OPTIONS），
/// 我们不响应 CORS，preflight 失败 → 实际请求不会发出。
/// text/plain 是简单请求不发 preflight，必须拒绝（防 CSRF）。
/// CLI 的 ureq 已显式设 Content-Type: application/json，不受影响。
async fn csrf_guard(req: Request, next: Next) -> Response {
    // 所有 /api 请求都校验 Host 头（防御纵深，防 DNS rebinding）
    let host = req
        .headers()
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !is_localhost_host(host) {
        return err_response(
            StatusCode::FORBIDDEN,
            PinErrorCode::InternalError,
            "host not allowed".to_string(),
        )
        .into_response();
    }

    // POST 请求额外校验 Content-Type
    if req.method() == Method::POST {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pin_actions::ShowPinError;

    // ---------- health ----------

    #[tokio::test]
    async fn test_health_returns_ok_with_version() {
        let Json(val) = health().await;
        assert_eq!(val["ok"], json!(true));
        assert_eq!(val["app"], json!("Agent Pin"));
        // version 字段存在且非空
        let version = val["version"].as_str().expect("version must be string");
        assert!(!version.is_empty());
    }

    // ---------- err_response ----------

    #[test]
    fn test_err_response_status_and_body() {
        let (status, Json(body)) = err_response(
            StatusCode::BAD_REQUEST,
            PinErrorCode::InvalidJson,
            "invalid JSON: unexpected token".to_string(),
        );
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["ok"], json!(false));
        assert_eq!(body["error"]["code"], json!("INVALID_JSON"));
        assert_eq!(
            body["error"]["message"],
            json!("invalid JSON: unexpected token")
        );
    }

    #[test]
    fn test_err_response_internal_error() {
        let (status, Json(body)) = err_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            PinErrorCode::InternalError,
            "disk full".to_string(),
        );
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"]["code"], json!("INTERNAL_ERROR"));
    }

    #[test]
    fn test_err_response_pin_not_found() {
        let (status, Json(body)) = err_response(
            StatusCode::NOT_FOUND,
            PinErrorCode::PinNotFound,
            "pin not found: pin_123".to_string(),
        );
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], json!("PIN_NOT_FOUND"));
    }

    // ---------- show_pin_err_response ----------

    #[test]
    fn test_show_pin_err_response_not_found() {
        let (status, Json(body)) =
            show_pin_err_response(ShowPinError::NotFound("pin not found: pin_123".into()));
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], json!("PIN_NOT_FOUND"));
        assert!(body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("pin_123"));
    }

    #[test]
    fn test_show_pin_err_response_failed() {
        let (status, Json(body)) =
            show_pin_err_response(ShowPinError::Failed("pin is in failed state".into()));
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"]["code"], json!("INTERNAL_ERROR"));
    }

    #[test]
    fn test_show_pin_err_response_window_create() {
        let (status, Json(body)) =
            show_pin_err_response(ShowPinError::WindowCreate("webview creation failed".into()));
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"]["code"], json!("WINDOW_CREATE_FAILED"));
    }

    #[test]
    fn test_show_pin_err_response_internal() {
        let (status, Json(body)) =
            show_pin_err_response(ShowPinError::Internal("set_state failed".into()));
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"]["code"], json!("INTERNAL_ERROR"));
    }

    // ---------- csrf_guard 中间件 ----------

    use axum::body::Body;
    use tower::ServiceExt;

    /// 构造带 csrf_guard 中间件的测试 Router。
    /// 不带 state（Router<()>），因为 csrf_guard 只读 headers 不需要 AppHandle。
    fn csrf_test_router() -> Router {
        Router::new()
            .route("/test", post(|| async { "ok" }).get(|| async { "ok" }))
            .layer(middleware::from_fn(csrf_guard))
    }

    /// 发送模拟请求到 csrf_test_router，返回响应。
    async fn send_csrf_request(method: Method, host: &str, content_type: Option<&str>) -> Response {
        let mut builder = Request::builder()
            .method(method)
            .uri("/test")
            .header("host", host);
        if let Some(ct) = content_type {
            builder = builder.header("content-type", ct);
        }
        csrf_test_router()
            .oneshot(builder.body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn test_csrf_guard_allows_json_post() {
        // 合法 POST：Host 白名单 + Content-Type: application/json → 放行
        let resp =
            send_csrf_request(Method::POST, "127.0.0.1:4317", Some("application/json")).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_csrf_guard_allows_json_post_localhost() {
        // localhost 也在 Host 白名单中
        let resp =
            send_csrf_request(Method::POST, "localhost:4317", Some("application/json")).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_csrf_guard_allows_custom_port() {
        // CLI --endpoint 自定义端口：Host 白名单允许任意端口的 localhost
        let resp =
            send_csrf_request(Method::POST, "127.0.0.1:9999", Some("application/json")).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_csrf_guard_allows_json_with_charset() {
        // application/json; charset=utf-8 是 RFC 7231 标准带 charset 的合法形式，
        // ureq/reqwest 默认会这样发。csrf_guard 用 starts_with 放行，此测试锁定该行为。
        let resp = send_csrf_request(
            Method::POST,
            "127.0.0.1:4317",
            Some("application/json; charset=utf-8"),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_csrf_guard_rejects_text_plain() {
        // text/plain 是简单请求（不发 preflight），必须拒绝防 CSRF
        let resp = send_csrf_request(Method::POST, "127.0.0.1:4317", Some("text/plain")).await;
        assert_eq!(resp.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    #[tokio::test]
    async fn test_csrf_guard_rejects_missing_content_type() {
        // 缺少 Content-Type 的 POST 必须拒绝
        let resp = send_csrf_request(Method::POST, "127.0.0.1:4317", None).await;
        assert_eq!(resp.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    #[tokio::test]
    async fn test_csrf_guard_rejects_bad_host_post() {
        // 非白名单 Host 拒绝 POST（防 DNS rebinding）
        let resp = send_csrf_request(Method::POST, "evil.com:4317", Some("application/json")).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_csrf_guard_rejects_bad_host_get() {
        // 非白名单 Host 也拒绝 GET（所有 /api 路由都校验 Host）
        let resp = send_csrf_request(Method::GET, "evil.com", None).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_csrf_guard_allows_get_good_host() {
        // 合法 Host 的 GET 放行
        let resp = send_csrf_request(Method::GET, "127.0.0.1:4317", None).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_csrf_guard_allows_get_localhost_custom_port() {
        // localhost 自定义端口的 GET 放行
        let resp = send_csrf_request(Method::GET, "localhost:8080", None).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_csrf_guard_rejects_empty_host() {
        // 空 Host（头缺失时 unwrap_or("")）也必须拒绝
        let resp = send_csrf_request(Method::POST, "", Some("application/json")).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_csrf_guard_rejects_empty_host_get() {
        // 空 Host 的 GET 也必须拒绝
        let resp = send_csrf_request(Method::GET, "", None).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    // ---------- is_localhost_host ----------

    #[test]
    fn test_is_localhost_host_valid() {
        assert!(is_localhost_host("127.0.0.1:4317"));
        assert!(is_localhost_host("localhost:4317"));
        assert!(is_localhost_host("LOCALHOST:4317"));
        assert!(is_localhost_host("127.0.0.1:9999"));
        assert!(is_localhost_host("localhost:80"));
        assert!(is_localhost_host("[::1]:4317"));
    }

    #[test]
    fn test_is_localhost_host_no_port() {
        assert!(is_localhost_host("127.0.0.1"));
        assert!(is_localhost_host("localhost"));
        assert!(is_localhost_host("::1"));
    }

    #[test]
    fn test_is_localhost_host_rejects_external() {
        assert!(!is_localhost_host("evil.com:4317"));
        assert!(!is_localhost_host("192.168.1.1:4317"));
        assert!(!is_localhost_host("example.com"));
        assert!(!is_localhost_host(""));
    }

    #[test]
    fn test_is_localhost_host_rejects_bad_port_syntax() {
        assert!(!is_localhost_host("127.0.0.1:abc"));
        assert!(!is_localhost_host("127.0.0.1:"));
        assert!(!is_localhost_host("localhost:99999"));
        assert!(!is_localhost_host("[::1]:abc"));
        assert!(!is_localhost_host("[::1]:"));
        assert!(!is_localhost_host("[::1]junk"));
        assert!(!is_localhost_host("[::1"));
    }

    // ---------- copy_image_blocks_to_store ----------

    use crate::pin::{ImageBlock, MarkdownBlock, PinDocument};
    use std::fs;

    /// 辅助：构造临时目录
    fn temp_dir(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "agent-pin-http-test-{}-{}",
            std::process::id(),
            name
        ));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn test_copy_image_copies_existing_file() {
        let tmp = temp_dir("copy-existing");
        let images_dir = tmp.join("images");
        let src = tmp.join("test.png");
        fs::write(&src, b"fake png").unwrap();

        let mut doc = PinDocument {
            version: 1,
            title: "Test".to_string(),
            blocks: vec![PinBlock::Image(ImageBlock {
                path: src.to_string_lossy().to_string(),
                caption: None,
            })],
            window: None,
            source: None,
            created_at: None,
        };

        copy_image_blocks_to_store_in(&mut doc, &images_dir).unwrap();

        if let PinBlock::Image(img) = &doc.blocks[0] {
            assert!(
                FsPath::new(&img.path).starts_with(&images_dir),
                "path should be rewritten to images dir, got: {}",
                img.path
            );
            assert!(img.path.ends_with(".png"), "should keep png extension");
            assert!(fs::metadata(&img.path).is_ok(), "copied file should exist");
        } else {
            panic!("expected image block");
        }
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_copy_image_preserves_path_for_missing_file() {
        let tmp = temp_dir("missing-file");
        let images_dir = tmp.join("images");
        let mut doc = PinDocument {
            version: 1,
            title: "Test".to_string(),
            blocks: vec![PinBlock::Image(ImageBlock {
                path: "/nonexistent/path/missing.png".to_string(),
                caption: None,
            })],
            window: None,
            source: None,
            created_at: None,
        };

        copy_image_blocks_to_store_in(&mut doc, &images_dir).unwrap();

        // 文件不存在，路径应保留原样
        if let PinBlock::Image(img) = &doc.blocks[0] {
            assert_eq!(img.path, "/nonexistent/path/missing.png");
        } else {
            panic!("expected image block");
        }
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_copy_image_skips_non_image_extension() {
        let tmp = temp_dir("skip-non-image");
        let images_dir = tmp.join("images");
        let src = tmp.join("test.bmp");
        fs::write(&src, b"fake bmp").unwrap();

        let mut doc = PinDocument {
            version: 1,
            title: "Test".to_string(),
            blocks: vec![PinBlock::Image(ImageBlock {
                path: src.to_string_lossy().to_string(),
                caption: None,
            })],
            window: None,
            source: None,
            created_at: None,
        };

        copy_image_blocks_to_store_in(&mut doc, &images_dir).unwrap();

        // 非图片扩展名，路径应保留原样
        if let PinBlock::Image(img) = &doc.blocks[0] {
            assert_eq!(img.path, src.to_string_lossy().to_string());
        }
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_copy_image_skips_relative_path() {
        let tmp = temp_dir("relative-path");
        let images_dir = tmp.join("images");
        let mut doc = PinDocument {
            version: 1,
            title: "Test".to_string(),
            blocks: vec![PinBlock::Image(ImageBlock {
                path: "relative/path.png".to_string(),
                caption: None,
            })],
            window: None,
            source: None,
            created_at: None,
        };

        copy_image_blocks_to_store_in(&mut doc, &images_dir).unwrap();

        // 相对路径不处理
        if let PinBlock::Image(img) = &doc.blocks[0] {
            assert_eq!(img.path, "relative/path.png");
        }
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_copy_image_only_affects_image_blocks() {
        let tmp = temp_dir("non-image-blocks");
        let images_dir = tmp.join("images");
        let mut doc = PinDocument {
            version: 1,
            title: "Test".to_string(),
            blocks: vec![
                PinBlock::Markdown(MarkdownBlock {
                    content: "## Hello".to_string(),
                }),
                PinBlock::Status(crate::pin::StatusBlock {
                    level: Some("info".to_string()),
                    text: "status".to_string(),
                }),
            ],
            window: None,
            source: None,
            created_at: None,
        };

        copy_image_blocks_to_store_in(&mut doc, &images_dir).unwrap();

        // 非 image block 不受影响
        assert_eq!(doc.blocks.len(), 2);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_copy_image_returns_error_when_images_dir_cannot_be_created() {
        let tmp = temp_dir("images-dir-file");
        let src = tmp.join("test.png");
        let images_dir = tmp.join("images");
        fs::write(&src, b"fake png").unwrap();
        fs::write(&images_dir, b"not a directory").unwrap();

        let mut doc = PinDocument {
            version: 1,
            title: "Test".to_string(),
            blocks: vec![PinBlock::Image(ImageBlock {
                path: src.to_string_lossy().to_string(),
                caption: None,
            })],
            window: None,
            source: None,
            created_at: None,
        };

        let err = copy_image_blocks_to_store_in(&mut doc, &images_dir).unwrap_err();
        assert!(err.contains("failed to create images dir"));
        if let PinBlock::Image(img) = &doc.blocks[0] {
            assert_eq!(img.path, src.to_string_lossy().to_string());
        }
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_missing_image_does_not_require_images_dir() {
        let tmp = temp_dir("missing-image-blocked-dir");
        let src = tmp.join("missing.png");
        let images_dir = tmp.join("images");
        fs::write(&images_dir, b"not a directory").unwrap();

        let mut doc = PinDocument {
            version: 1,
            title: "Test".to_string(),
            blocks: vec![PinBlock::Image(ImageBlock {
                path: src.to_string_lossy().to_string(),
                caption: None,
            })],
            window: None,
            source: None,
            created_at: None,
        };

        copy_image_blocks_to_store_in(&mut doc, &images_dir).unwrap();
        if let PinBlock::Image(img) = &doc.blocks[0] {
            assert_eq!(img.path, src.to_string_lossy().to_string());
        }
        let _ = fs::remove_dir_all(&tmp);
    }
}
