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
// 边界说明（见 docs/phase-plan.md）：
// - 关闭 Pin 窗口 = destroy 窗口 + state=hidden（可恢复）
// - 删除 Pin = destroy 窗口 + 删 pins/{pinId}.json + 从 state.json 移除（不可恢复）
// - 托盘 Quit = 退出应用 + 停止 HTTP

mod http;
mod pin;
mod registry;
mod storage;
mod tray;
mod window;

use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

use crate::storage::{PinMeta, PinState};

// ---------- invoke 命令 ----------

/// 前端渲染入口：按 pinId 读取 PinDocument。
/// 窗口 URL 只带 pinId，数据走这条命令，避免把完整 JSON 塞进 URL。
#[tauri::command]
fn get_pin_document(pin_id: String) -> Option<serde_json::Value> {
    registry::REGISTRY
        .get(&pin_id)
        .and_then(|doc| serde_json::to_value(doc).ok())
}

/// 管理界面：列出所有 Pin 元数据（按 createdAt 降序）。
#[tauri::command]
fn list_pins() -> Vec<PinMeta> {
    registry::REGISTRY.list()
}

/// 管理界面/托盘：显示 Pin（创建窗口）。
/// 幂等：已 visible 直接返回 Ok。
#[tauri::command]
fn show_pin(app: tauri::AppHandle, pin_id: String) -> Result<(), String> {
    let meta = registry::REGISTRY
        .get_meta(&pin_id)
        .ok_or_else(|| format!("pin not found: {}", pin_id))?;

    if meta.state == PinState::Visible {
        return Ok(()); // 幂等
    }

    // 清理可能的孤儿窗口（state=hidden 但窗口存在）
    if let Err(e) = window::hide_pin_window(&app, &pin_id) {
        eprintln!("[agent-pin] show_pin cleanup: {}", e);
    }

    let doc = registry::REGISTRY
        .get(&pin_id)
        .ok_or_else(|| format!("pin doc missing: {}", pin_id))?;

    window::create_pin_window(&app, &pin_id, &doc)?;
    registry::REGISTRY.set_state(&pin_id, PinState::Visible)?;
    tray::refresh(&app);
    Ok(())
}

/// 管理界面/托盘：隐藏 Pin（destroy 窗口 + state=hidden）。
/// 幂等：已 hidden 直接返回 Ok。
#[tauri::command]
fn hide_pin(app: tauri::AppHandle, pin_id: String) -> Result<(), String> {
    let meta = registry::REGISTRY
        .get_meta(&pin_id)
        .ok_or_else(|| format!("pin not found: {}", pin_id))?;

    if meta.state == PinState::Hidden {
        return Ok(()); // 幂等
    }

    window::hide_pin_window(&app, &pin_id)?;
    registry::REGISTRY.set_state(&pin_id, PinState::Hidden)?;
    tray::refresh(&app);
    Ok(())
}

/// 管理界面/托盘：隐藏所有可见 Pin。
#[tauri::command]
fn hide_all_pins(app: tauri::AppHandle) -> Result<(), String> {
    let metas = registry::REGISTRY.list();
    for meta in metas {
        if meta.state != PinState::Visible {
            continue;
        }
        if let Err(e) = window::hide_pin_window(&app, &meta.pin_id) {
            eprintln!("[agent-pin] hide_all window for {}: {}", meta.pin_id, e);
        }
        if let Err(e) = registry::REGISTRY.set_state(&meta.pin_id, PinState::Hidden) {
            eprintln!("[agent-pin] hide_all state for {}: {}", meta.pin_id, e);
        }
    }
    tray::refresh(&app);
    Ok(())
}

/// 管理界面：删除 Pin（不可恢复）。
/// 1. 销毁窗口（如果存在）
/// 2. 删除 registry entry + pins/{pinId}.json + 更新 state.json
#[tauri::command]
fn delete_pin(app: tauri::AppHandle, pin_id: String) -> Result<(), String> {
    // 1. 销毁窗口（如果存在）
    if let Err(e) = window::hide_pin_window(&app, &pin_id) {
        eprintln!("[agent-pin] delete_pin window: {}", e);
    }
    // 2. 删除 registry entry + 文件 + state
    registry::REGISTRY.remove(&pin_id)?;
    tray::refresh(&app);
    Ok(())
}

/// 管理界面：打开数据目录（跨平台）。
/// Windows: explorer；macOS: open；Linux: xdg-open。
/// 用 std::process::Command 而非 shell plugin，避免 scope 配置复杂度。
#[tauri::command]
fn open_data_dir() -> Result<(), String> {
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

// ---------- 应用入口 ----------

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // 1. 初始化数据目录 + 加载历史 Pin
            //    init 失败不阻塞启动：持久化失败时仍可创建 Pin（只是不持久化）
            if let Err(e) = storage::init() {
                eprintln!(
                    "[agent-pin] storage init failed (pins will not persist): {}",
                    e
                );
            }
            registry::REGISTRY.load_from_disk();

            // 2. 同步绑定 HTTP 端口 + spawn HTTP server
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

            // 3. 系统托盘（委托 tray 模块）
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
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_pin_document,
            list_pins,
            show_pin,
            hide_pin,
            hide_all_pins,
            delete_pin,
            open_data_dir,
        ])
        .on_window_event(|window, event| {
            // 窗口销毁事件：只在 state=visible 时设 hidden。
            // 避免与 hide 路由、show 路由清理孤儿窗口、delete_pin 冲突。
            // 管理界面窗口 label="manager"，不在 registry，忽略。
            if let tauri::WindowEvent::Destroyed = event {
                let pin_id = window.label();
                if pin_id == "manager" {
                    return;
                }
                if let Some(meta) = registry::REGISTRY.get_meta(pin_id) {
                    if meta.state == PinState::Visible {
                        if let Err(e) =
                            registry::REGISTRY.set_state(pin_id, PinState::Hidden)
                        {
                            eprintln!("[agent-pin] on_window_event set_state: {}", e);
                        }
                        // 状态变化，刷新托盘菜单
                        tray::refresh(window.app_handle());
                    }
                }
                // entry 不存在（已被 delete_pin remove）：忽略
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
