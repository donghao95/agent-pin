// Agent Pin 桌面应用 - Rust 后端入口
//
// Phase 2-B 职责：
// 1. 启动时初始化数据目录 + 从磁盘加载历史 Pin
// 2. 启动本地 HTTP 服务（127.0.0.1:4317）
// 3. 注册 invoke 命令：get_pin_document / list_pins / show_pin / hide_pin /
//    hide_all_pins / delete_pin / get_data_dir（管理界面 + 托盘共用）
// 4. 系统托盘（委托 tray 模块）：Quit + 最近 5 hidden Pin 快恢 + 管理界面 + 隐藏全部
// 5. 窗口销毁事件：state=visible 时设 hidden（关闭=hidden，不删除记录）
//
// 边界说明（见 docs/06_phase_plan.md）：
// - 关闭 Pin 窗口 = destroy 窗口 + state=hidden（可恢复）
// - 删除 Pin = destroy 窗口 + 删 pins/{pinId}.json + 从 state.json 移除（不可恢复）
// - 托盘 Quit = 退出应用 + 停止 HTTP

mod http;
mod manager_snapshot;
mod pin;
mod pin_actions;
mod registry;
mod storage;
mod tray;
mod updater;
mod window;

use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

use crate::pin_actions::ShowPinMode;
use crate::storage::{PinMeta, PinState};

// ---------- invoke 命令权限策略 ----------
//
// 最小权限原则（防 WebView XSS 横向攻击）：
// - 管理类命令（show/hide/hide-all/delete/open_data_dir）：仅 manager 窗口可调用
// - Pin 自身命令（close/fit/remember）：仅 label==pinId 的 pin 窗口可调用
// - 数据读取（get_pin_document）：pin 窗口只能读自身，manager 不受限
// - 列表（list_pins）：仅 manager 窗口可调用
// - 外链打开（open_external_url）：仅 pin 窗口可调用（manager 不应有外链场景）
// - 更新检查（check_for_updates）：不限窗口（manager 和托盘共用）

/// 校验调用窗口是否为 manager。
fn is_manager_label(label: &str) -> bool {
    label == "manager"
}

fn is_pin_window_label(label: &str) -> bool {
    label.starts_with("pin_")
}

fn is_pin_owner_label(label: &str, pin_id: &str) -> bool {
    is_pin_window_label(label) && label == pin_id
}

fn can_read_pin_document(label: &str, pin_id: &str) -> bool {
    is_manager_label(label) || is_pin_owner_label(label, pin_id)
}

fn require_manager(window: &tauri::WebviewWindow) -> Result<(), String> {
    if !is_manager_label(window.label()) {
        return Err("forbidden: only manager window can call this command".to_string());
    }
    Ok(())
}

/// 校验调用窗口是否为指定 pinId 的 pin 窗口（label 以 pin_ 开头且 == pin_id）。
fn require_pin_owner(window: &tauri::WebviewWindow, pin_id: &str) -> Result<(), String> {
    let label = window.label();
    if !is_pin_owner_label(label, pin_id) {
        return Err(format!(
            "forbidden: window '{}' cannot operate on pin '{}'",
            label, pin_id
        ));
    }
    Ok(())
}

// ---------- invoke 命令 ----------

/// 前端渲染入口：按 pinId 读取 PinDocument。
/// 窗口 URL 只带 pinId，数据走这条命令，避免把完整 JSON 塞进 URL。
/// m5：Pin 窗口（label 以 pin_ 开头）只能读取自身的 PinDocument，
/// manager 窗口可读取任意 PinDocument。其他窗口一律拒绝，防未来 WebView 扩展绕过。
#[tauri::command]
fn get_pin_document(window: tauri::WebviewWindow, pin_id: String) -> Option<serde_json::Value> {
    if !can_read_pin_document(window.label(), &pin_id) {
        return None;
    }
    registry::REGISTRY
        .get(&pin_id)
        .and_then(|doc| serde_json::to_value(doc).ok())
}

/// 管理界面：获取 ManagerSnapshot（Manager 列表权威数据源）。
/// 仅 manager 窗口可调用。用于 mount/focus 时的主动拉取，配合 manager:snapshot 推送。
#[tauri::command]
fn get_manager_snapshot(
    window: tauri::WebviewWindow,
) -> Result<manager_snapshot::ManagerSnapshot, String> {
    require_manager(&window)?;
    Ok(manager_snapshot::build())
}

/// 管理界面/托盘：显示 Pin（创建窗口）。
/// 仅 manager 窗口可调用（管理类命令）。
#[tauri::command]
fn show_pin(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    pin_id: String,
) -> Result<(), String> {
    require_manager(&window)?;
    pin_actions::show_pin(&app, &pin_id, ShowPinMode::AsyncCreate).map_err(|e| e.to_string())
}

/// 管理界面/托盘：隐藏 Pin（destroy 窗口 + state=hidden）。
/// 仅 manager 窗口可调用（管理类命令）。
///
/// 流程与 close_pin 一致（M-1/M-2 修复）：
/// - state=Hidden 但窗口仍在：仍尝试销毁（兜底卡住的窗口）
/// - 先 set_state_quiet(Hidden) → destroy → 成功统一 emit；失败检查窗口是否存在再决定回滚
///
/// 返回 ManagerSnapshot：前端直接 applySnapshot。
#[tauri::command]
fn hide_pin(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    pin_id: String,
) -> Result<manager_snapshot::ManagerSnapshot, String> {
    require_manager(&window)?;
    let meta = registry::REGISTRY
        .get_meta(&pin_id)
        .ok_or_else(|| format!("pin not found: {}", pin_id))?;

    // M-1 修复：state=Hidden 但窗口仍在时仍尝试销毁（与 close_pin 一致）
    if meta.state == PinState::Hidden {
        if app.get_webview_window(&pin_id).is_some() {
            let _ = window::hide_pin_window(&app, &pin_id);
        }
        return Ok(manager_snapshot::build());
    }

    // M-2 修复：先 set_state_quiet(Hidden) → 后 destroy（与 close_pin 一致）
    // destroy 失败时检查窗口是否存在再决定回滚，避免 Destroyed 异步竞态
    registry::REGISTRY.set_state_quiet(&pin_id, PinState::Hidden)?;
    match window::hide_pin_window(&app, &pin_id) {
        Ok(()) => {
            registry::emit_changed();
            manager_snapshot::emit_manager_snapshot(&app);
        }
        Err(e) => {
            let window_still_exists = app.get_webview_window(&pin_id).is_some();
            eprintln!(
                "[agent-pin] hide_pin destroy failed for {}: {} (window_still_exists={})",
                pin_id, e, window_still_exists
            );
            if window_still_exists {
                if let Err(rollback_err) =
                    registry::REGISTRY.set_state_quiet(&pin_id, PinState::Visible)
                {
                    eprintln!(
                        "[agent-pin] hide_pin rollback state failed for {}: {}",
                        pin_id, rollback_err
                    );
                }
            }
            registry::emit_changed();
            manager_snapshot::emit_manager_snapshot(&app);
            return Err(format!("failed to hide pin window: {}", e));
        }
    }
    Ok(manager_snapshot::build())
}

/// 管理界面/托盘：隐藏所有可见 Pin。
/// 仅 manager 窗口可调用（管理类命令）。
/// M10 修复：复用 pin_actions::hide_all_visible，消除三份拷贝。
///
/// 返回 ManagerSnapshot：前端直接 applySnapshot。
#[tauri::command]
fn hide_all_pins(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<manager_snapshot::ManagerSnapshot, String> {
    require_manager(&window)?;
    let failed = pin_actions::hide_all_visible(&app);
    // hide_all_visible 已统一 emit pins:changed，额外 emit manager:snapshot
    manager_snapshot::emit_manager_snapshot(&app);
    if failed.is_empty() {
        Ok(manager_snapshot::build())
    } else {
        Err(format!(
            "failed to hide {} pin(s): {}",
            failed.len(),
            failed.join(", ")
        ))
    }
}

/// 管理界面：删除 Pin（不可恢复）。
/// 仅 manager 窗口可调用（管理类命令）。
/// C1 修复：先校验 pin_id 存在于 registry，再销毁窗口。
/// M3 修复：窗口销毁失败不静默吞错，直接返回 Err。
///
/// 返回 ManagerSnapshot：前端直接 applySnapshot。
#[tauri::command]
fn delete_pin(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    pin_id: String,
) -> Result<manager_snapshot::ManagerSnapshot, String> {
    require_manager(&window)?;
    // 1. 先校验 pin_id 存在性（防销毁 manager 等非 Pin 窗口）
    registry::REGISTRY
        .get_meta(&pin_id)
        .ok_or_else(|| format!("pin not found: {}", pin_id))?;
    // 2. 销毁窗口（失败直接返回 Err，不静默吞错）
    window::hide_pin_window(&app, &pin_id)?;
    // 3. 删除 registry entry + 文件 + state（remove 内部 emit pins:changed）
    registry::REGISTRY.remove(&pin_id)?;
    // 额外 emit manager:snapshot 让 Manager 列表同步
    manager_snapshot::emit_manager_snapshot(&app);
    Ok(manager_snapshot::build())
}

/// 管理界面：打开数据目录（跨平台）。
/// 仅 manager 窗口可调用（管理类命令）。
#[tauri::command]
fn open_data_dir(window: tauri::WebviewWindow) -> Result<(), String> {
    require_manager(&window)?;
    let dir = storage::data_dir();
    #[cfg(target_os = "windows")]
    let cmd = "explorer";
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(target_os = "linux")]
    let cmd = "xdg-open";
    std::process::Command::new(cmd)
        .arg(&dir)
        .spawn()
        .map_err(|e| format!("failed to open data dir: {}", e))?;
    Ok(())
}

/// 检查更新：调 GitHub API 查最新 release，与当前版本对比。
/// 不限窗口（manager 和托盘共用）。
/// force=true 时跳过 24h 缓存强制请求。
/// 失败返回 Err（前端/托盘决定是否提示）。
#[tauri::command]
async fn check_for_updates(force: bool) -> Result<updater::UpdateCheckResult, String> {
    tauri::async_runtime::spawn_blocking(move || updater::check(force))
        .await
        .map_err(|e| format!("join handle: {}", e))?
}

/// Pin 窗口自适应高度：前端渲染后测量内容高度，通知后端调整窗口高度。
/// 仅 pin 窗口可调用（label == pin_id）。
/// 见 window::fit_pin_window_height 文档。
#[tauri::command]
fn fit_pin_window_height(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    pin_id: String,
    content_height: f64,
) -> Result<bool, String> {
    require_pin_owner(&window, &pin_id)?;
    window::fit_pin_window_height(&app, &pin_id, content_height)
}

/// 关闭 Pin 窗口（正常关闭路径，由 Pin 窗口 ESC / 关闭按钮触发）。
/// 仅 pin 窗口可调用（label == pin_id）。
///
/// 流程：set_state_quiet(Hidden) → destroy 窗口 → 成功统一 emit；失败回滚 state。
///
/// 时序设计（第一性原理）：
/// - 用 quiet 版本先改 state，不立即 emit，避免在 destroy 完成前把中间态推给 Manager
/// - destroy 成功后统一 emit（pins:changed 给 tray + manager:snapshot 给 Manager）
/// - destroy 失败则 set_state_quiet(Visible) 回滚 + emit，让用户知道关闭失败，不假装 hidden
///
/// 幂等：已 Hidden 且窗口已销毁时返回 Ok（不重复 emit）。若 state==Hidden 但窗口仍在
///（上次 destroy 失败或窗口卡住），仍尝试销毁窗口，让用户能关闭卡住的窗口。
#[tauri::command]
fn close_pin(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    pin_id: String,
) -> Result<(), String> {
    require_pin_owner(&window, &pin_id)?;
    // m1：入口处校验 pin_id 格式（防路径穿越），与 remove / remember_window_size 一致。
    crate::storage::validate_pin_id(&pin_id)?;
    let meta = registry::REGISTRY
        .get_meta(&pin_id)
        .ok_or_else(|| format!("pin not found: {}", pin_id))?;

    // 已 Hidden：幂等返回，但若窗口仍在（上次 destroy 失败或窗口卡住）仍尝试销毁。
    if meta.state == PinState::Hidden {
        if app.get_webview_window(&pin_id).is_some() {
            if let Err(e) = window::hide_pin_window(&app, &pin_id) {
                eprintln!("[agent-pin] close_pin destroy failed for {}: {}", pin_id, e);
            }
        }
        return Ok(()); // 不重复 emit（state 无变更）
    }

    // 先 quiet 改 state=Hidden（不 emit，避免 destroy 完成前推中间态）
    registry::REGISTRY.set_state_quiet(&pin_id, PinState::Hidden)?;
    // 再销毁窗口（触发 Destroyed，但 state 已 Hidden，handler 跳过）
    match window::hide_pin_window(&app, &pin_id) {
        Ok(()) => {
            // destroy 成功：统一 emit（tray 用 pins:changed，Manager 用 manager:snapshot）
            registry::emit_changed();
            manager_snapshot::emit_manager_snapshot(&app);
        }
        Err(e) => {
            // M7 修复：destroy 失败时检查窗口是否真的还在，避免 Destroyed 异步竞态。
            // - 窗口还在（get_webview_window 返回 Some）：回滚 Visible，让用户知道关闭失败可重试。
            //   此时 Destroyed 不会触发（窗口没销毁），state=Visible 一致。
            // - 窗口不在（get_webview_window 返回 None）：维持 Hidden，不回滚。
            //   窗口已销毁，Destroyed 可能已触发或即将触发，维持 Hidden 与窗口事实一致。
            //   若回滚 Visible，Destroyed handler 看到 state=Visible 会 set_state(Hidden)，
            //   导致中间态 snapshot 错误（先推 visible 再推 hidden）。
            let window_still_exists = app.get_webview_window(&pin_id).is_some();
            eprintln!(
                "[agent-pin] close_pin destroy failed for {}: {} (window_still_exists={})",
                pin_id, e, window_still_exists
            );
            if window_still_exists {
                if let Err(rollback_err) =
                    registry::REGISTRY.set_state_quiet(&pin_id, PinState::Visible)
                {
                    eprintln!(
                        "[agent-pin] close_pin rollback state failed for {}: {}",
                        pin_id, rollback_err
                    );
                }
            }
            // state 已是正确值（窗口在→Visible，窗口不在→Hidden），统一 emit
            registry::emit_changed();
            manager_snapshot::emit_manager_snapshot(&app);
            return Err(format!("failed to close pin window: {}", e));
        }
    }
    Ok(())
}

/// 记录用户手动调整后的窗口尺寸（持久化到 state.json）。
/// 仅 pin 窗口可调用（label == pin_id）。
/// show 时优先用此尺寸恢复窗口。
/// 仅前端检测到用户手动 resize 后调用，fit_pin_window_height 的自动调整不调用。
#[tauri::command]
fn remember_pin_size(
    window: tauri::WebviewWindow,
    pin_id: String,
    width: f64,
    height: f64,
) -> Result<(), String> {
    require_pin_owner(&window, &pin_id)?;
    registry::REGISTRY.remember_window_size(&pin_id, width, height)
}

/// 用系统默认浏览器打开外链。
/// 仅 pin 窗口可调用（manager 不应有外链场景）。
///
/// Tauri 2 WebView 中 `<a target="_blank">` 不会自动打开系统浏览器（被 WebView 拦截），
/// 必须主动拦截链接点击并用 opener plugin 打开。
///
/// 安全：协议白名单只允许 http/https/mailto，拒绝 file:// 等危险协议
///（防 file:// 打开本地文件、javascript: 执行代码等）。
#[tauri::command]
fn open_external_url(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    url: String,
) -> Result<(), String> {
    // 仅 pin 窗口可调用
    if !is_pin_window_label(window.label()) {
        return Err("forbidden: only pin windows can open external urls".to_string());
    }
    use tauri_plugin_opener::OpenerExt;
    // 协议白名单校验
    let scheme = url.split(':').next().unwrap_or("").to_lowercase();
    match scheme.as_str() {
        "http" | "https" | "mailto" => {}
        other => return Err(format!("unsupported url scheme: {}", other)),
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| format!("failed to open url: {}", e))
}

// ---------- 应用入口 ----------

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        // 单实例检查：第二次启动时唤起已有实例（打开管理界面 + 聚焦），然后自身退出。
        // 必须在 setup 之前注册。放在所有 plugin 之后、setup 之前。
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Err(e) = tray::open_manager_window(app) {
                eprintln!("[agent-pin] single instance open manager: {}", e);
            }
        }))
        .setup(|app| {
            // 1. 注入 AppHandle 到 registry（必须在 load_from_disk 之前，
            //    确保 registry 后续 insert/set_state/remove 能 emit 事件）
            registry::set_app_handle(app.handle().clone());

            // 2. 初始化数据目录 + 加载历史 Pin
            //    init 失败不阻塞启动：持久化失败时仍可创建 Pin（只是不持久化）
            if let Err(e) = storage::init() {
                eprintln!(
                    "[agent-pin] storage init failed (pins will not persist): {}",
                    e
                );
            }
            registry::REGISTRY.load_from_disk();

            // 3. 同步绑定 HTTP 端口 + spawn HTTP server
            //    bind 在 setup hook 中同步执行，失败时直接弹窗 + 退出（与 tray build 失败一致）。
            //    不用 channel 是因为 setup hook 不能 await，同步 bind + 把 listener 传给
            //    async task 最简单可靠。bind 成功后 listener 已占用端口，不会出现"端口被抢"的窗口期。
            let std_listener = match std::net::TcpListener::bind("127.0.0.1:4317") {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("[agent-pin] failed to bind 127.0.0.1:4317: {}", e);
                    let app_handle = app.handle().clone();
                    app_handle
                        .dialog()
                        .message(format!(
                            "HTTP 端口 4317 绑定失败：{}\n\n请确认 Agent Pin 未重复启动，且端口未被占用。\nAgent Pin 将退出。",
                            e
                        ))
                        .title("Agent Pin 启动失败")
                        .show(move |_| {
                            app_handle.exit(1);
                        });
                    // 不返回 Err 避免 Tauri panic；dialog 关闭后回调 exit(1)
                    return Ok(());
                }
            };
            // tokio 的 from_std 要求 listener 非阻塞
            let _ = std_listener.set_nonblocking(true);

            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                http::start_http(app_handle, std_listener).await;
            });

            // 4. 系统托盘（委托 tray 模块）
            //    托盘是 MVP 核心 UI 入口（无主窗口，用户靠托盘退出），构建失败必须弹窗 + 退出。
            //    不返回 Err 避免 Tauri panic；用 dialog show 回调中 app.exit(1)，对话框关闭后退出。
            if let Err(e) = tray::build(app.handle()) {
                eprintln!("[agent-pin] tray build failed: {}", e);
                let app_handle = app.handle().clone();
                app_handle
                    .dialog()
                    .message(format!(
                        "系统托盘构建失败：{}\n\nAgent Pin 将退出。",
                        e
                    ))
                    .title("Agent Pin 启动失败")
                    .show(move |_| {
                        app_handle.exit(1);
                    });
                // 托盘构建失败后不再执行后续步骤（更新检查会调 tray::refresh，
                // 但 tray 不存在只会 eprintln + 浪费一次网络请求）
                return Ok(());
            }

            // 4. 启动时自动打开管理界面
            //    用户启动 app 后能直接看到管理界面，不用先点托盘。
            //    失败只 eprintln，不阻塞应用（托盘仍可用，用户可手动打开）。
            if let Err(e) = tray::open_manager_window(app.handle()) {
                eprintln!("[agent-pin] startup open manager: {}", e);
            }

            // 5. 启动时静默检查更新（异步、不阻塞、失败忽略）
            //    缓存命中（24h 内）时不会实际请求 GitHub API。
            //    无论是否有新版本都刷新托盘：缓存可能已更新（has_update=false 时也写缓存），
            //    托盘菜单应反映最新检查结果。检查失败时不刷新（无新信息）。
            let app_handle_for_update = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                match tauri::async_runtime::spawn_blocking(|| updater::check(false)).await {
                    Ok(Ok(_result)) => {
                        tray::refresh(&app_handle_for_update);
                    }
                    Ok(Err(e)) => eprintln!("[agent-pin] startup update check: {}", e),
                    Err(e) => eprintln!("[agent-pin] startup update check join: {}", e),
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_pin_document,
            get_manager_snapshot,
            show_pin,
            hide_pin,
            hide_all_pins,
            delete_pin,
            open_data_dir,
            check_for_updates,
            fit_pin_window_height,
            close_pin,
            remember_pin_size,
            open_external_url,
        ])
        .on_window_event(|window, event| {
            // 管理界面窗口关闭按钮：拦截 close，改为 hide（缩回托盘，不 destroy）。
            // 用户点托盘"打开管理界面"重新 show。
            // 只有托盘"退出 Agent Pin"（app.exit(0)）才真正退出 app。
            // Pin 窗口不拦截：关闭=destroy+state=hidden（Phase 2-B 设计，可恢复）。
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "manager" {
                    api.prevent_close();
                    if let Err(e) = window.hide() {
                        eprintln!("[agent-pin] manager hide on close: {}", e);
                    }
                    return;
                }
            }

            // Pin 窗口销毁事件：作为异常兜底，确保窗口被销毁后 state 最终为 hidden。
            //
            // 正常关闭路径（ESC / 关闭按钮 / hide_pin 命令）：
            //   前端 invoke close_pin → set_state(Hidden) + emit + destroy
            //   → Destroyed 触发时 state 已是 Hidden，下方 `if meta.state == Visible` 守卫跳过。
            //   → 管理页刷新由 close_pin 的 emit 同步触发，不依赖 Destroyed 时序。
            //
            // 异常销毁路径（窗口崩溃等）：
            //   Destroyed 触发时 state 仍为 Visible → set_state(Hidden) + emit → 管理页刷新。
            //
            // show_pin 重建路径：
            //   show_pin 在 cleanup 前 mark_recreating(pin_id)。
            //   cleanup 触发 Destroyed → is_recreating=true → unmark + return（跳过 state 更新）。
            //   新窗口创建后 set_state(Visible) 不被覆盖。
            //
            // 旧方案用 get_webview_window(pin_id).is_some() 判断是否重建，
            // 但 Tauri 2 Destroyed 触发时窗口可能未从内部管理器注销，返回 Some（正在销毁的窗口本身），
            // 导致正常关闭也被误判为重建，跳过 set_state(Hidden)，不 emit pins:changed，管理页不刷新。
            // 新方案用显式 recreating 标志，不依赖 Tauri 内部时序。
            if let tauri::WindowEvent::Destroyed = event {
                let pin_id = window.label();
                if pin_id == "manager" {
                    return;
                }
                // 重建中的 cleanup：跳过 state 更新，清除标志
                if registry::REGISTRY.is_recreating(pin_id) {
                    registry::REGISTRY.unmark_recreating(pin_id);
                    return;
                }
                // 兜底：窗口异常销毁时确保 state=Hidden（正常关闭时 state 已是 Hidden，守卫跳过）
                if let Some(meta) = registry::REGISTRY.get_meta(pin_id) {
                    if meta.state == PinState::Visible {
                        if let Err(e) =
                            registry::REGISTRY.set_state(pin_id, PinState::Hidden)
                        {
                            eprintln!("[agent-pin] on_window_event set_state: {}", e);
                        }
                        // set_state 已 emit pins:changed（tray 刷新），
                        // 额外 emit manager:snapshot 让 Manager 列表同步
                        manager_snapshot::emit_manager_snapshot(&window.app_handle());
                    }
                }
                // entry 不存在（已被 delete_pin remove）：忽略
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

// ---------- invoke 权限测试 ----------

#[cfg(test)]
mod tests {
    use super::{can_read_pin_document, is_manager_label, is_pin_owner_label, is_pin_window_label};

    #[test]
    fn test_manager_label_check() {
        assert!(is_manager_label("manager"));
        assert!(!is_manager_label("pin_123_000001"));
        assert!(!is_manager_label("other"));
        assert!(!is_manager_label(""));
    }

    #[test]
    fn test_pin_owner_label_check() {
        assert!(is_pin_owner_label("pin_123_000001", "pin_123_000001"));
        // label 不以 pin_ 开头
        assert!(!is_pin_owner_label("manager", "pin_123_000001"));
        // label != pin_id
        assert!(!is_pin_owner_label("pin_123_000001", "pin_456_000001"));
        // label 不以 pin_ 开头但内容匹配（不应通过）
        assert!(!is_pin_owner_label("pinx_123_000001", "pinx_123_000001"));
    }

    #[test]
    fn test_open_external_url_requires_pin_window() {
        assert!(is_pin_window_label("pin_123_000001"));
        assert!(!is_pin_window_label("manager"));
        assert!(!is_pin_window_label("other"));
    }

    #[test]
    fn test_get_pin_document_permission_check() {
        assert!(can_read_pin_document("manager", "pin_123_000001"));
        assert!(can_read_pin_document("pin_123_000001", "pin_123_000001"));
        assert!(!can_read_pin_document("pin_123_000001", "pin_456_000001"));
        assert!(!can_read_pin_document("settings", "pin_123_000001"));
        assert!(!can_read_pin_document("plugin", "pin_123_000001"));
        assert!(!can_read_pin_document("", "pin_123_000001"));
    }
}
