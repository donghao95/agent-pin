// Agent Pin 桌面应用 - Rust 后端入口
//
// Phase 1 职责：
// 1. 启动本地 HTTP 服务（127.0.0.1:4317），用 tauri::async_runtime::spawn
// 2. 注册 invoke 命令 get_pin_document，供前端按 pinId 读取渲染数据
// 3. 最小系统托盘：仅 "Quit Agent Pin"，不做 Pin 历史恢复
// 4. 窗口销毁时从内存 registry 移除（Phase 1 关闭=销毁，不承诺恢复）
//
// 边界说明（见 docs/phase-plan.md）：
// - 托盘只负责应用生命周期，不负责 Pin 生命周期
// - 关闭 Pin = 销毁窗口 + 从 registry 移除
// - 托盘 Quit = 退出应用 + 停止 HTTP

mod http;
mod pin;
mod registry;
mod window;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;

/// 前端 invoke 入口：按 pinId 读取 PinDocument。
/// 窗口 URL 只带 pinId，数据走这条命令，避免把完整 JSON 塞进 URL。
#[tauri::command]
fn get_pin_document(pin_id: String) -> Option<serde_json::Value> {
    registry::REGISTRY
        .get(&pin_id)
        .and_then(|doc| serde_json::to_value(doc).ok())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            // 1. 启动 HTTP server
            //    用 tauri::async_runtime::spawn 而非直接 tokio::spawn，
            //    以和 Tauri 自身的 runtime 协调（Tauri 2 默认就是 tokio，但走抽象层更安全）。
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                http::start_http(app_handle).await;
            });

            // 2. 最小系统托盘：仅 Quit Agent Pin
            let quit_item =
                MenuItem::with_id(app, "quit", "Quit Agent Pin", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&quit_item])?;
            let _tray = TrayIconBuilder::new()
                .icon(
                    app.default_window_icon()
                        .cloned()
                        .expect("default window icon missing; check tauri.conf.json bundle.icon"),
                )
                .tooltip("Agent Pin")
                .menu(&menu)
                .on_menu_event(|app, event| {
                    if event.id.as_ref() == "quit" {
                        app.exit(0);
                    }
                })
                .build(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_pin_document])
        .on_window_event(|window, event| {
            // 窗口销毁时从 registry 移除。
            // Phase 1 关闭=销毁，不承诺恢复；Phase 2 会改成 hidden + 文件存储。
            if let tauri::WindowEvent::Destroyed = event {
                let pin_id = window.label();
                registry::REGISTRY.remove(pin_id);
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
