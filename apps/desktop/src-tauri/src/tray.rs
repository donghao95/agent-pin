// 系统托盘
//
// Phase 2-B 职责：
// 1. 最近 5 个 hidden Pin 快恢（点击即 show）
// 2. 打开管理界面（完整历史 + 搜索 + 删除）
// 3. 隐藏全部可见 Pin
// 4. 检查更新（调 GitHub API，有新版打开浏览器）
// 5. 退出 Agent Pin
//
// 菜单刷新机制：
// registry 在 insert/set_state/remove 成功后 emit "pins:changed" 事件，
// tray 在 build 时 listen 该事件，收到后自动调 refresh 重建菜单。
// 调用方（HTTP handler / invoke command / 菜单点击）不再需要手动调 tray::refresh。
// Tauri 2 没提供"菜单即将显示时重建"的回调，所以必须主动 set_menu。
//
// 契约来源：docs/06_phase_plan.md Phase 2-B、docs/01_product_spec.md §13

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Listener, Manager, WebviewUrl, WebviewWindowBuilder,
};
use tauri_plugin_dialog::DialogExt;

use crate::pin_actions::ShowPinMode;
use crate::registry;
use crate::updater;

/// 托盘 id（用于 tray_by_id 获取后刷新菜单）
const TRAY_ID: &str = "main";
/// 托盘快恢列表最多显示多少个 hidden Pin
const RECENT_LIMIT: usize = 5;
/// 托盘菜单项标题最大字符数（中文按 chars 截断）
const TITLE_MAX_CHARS: usize = 30;
/// 托盘"检查更新"菜单项 id
const MENU_CHECK_UPDATE: &str = "check_update";

/// M11 修复：防止"检查更新"并发触发。
/// 用户连续点击时，第二次及以后的点击直接忽略，避免重复网络请求和重复弹窗。
static UPDATE_CHECK_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

/// 构建系统托盘。在 Tauri setup hook中调用。
///
/// 同时注册 listen "pins:changed" 事件：registry 在 Pin 状态变更后 emit 该事件，
/// tray 收到后自动 refresh 重建菜单。监听器存活到应用退出（Tauri 2 listen 语义）。
pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app)?;
    let _tray = TrayIconBuilder::with_id(TRAY_ID)
        .icon(
            app.default_window_icon()
                .cloned()
                .expect("default window icon missing; check tauri.conf.json bundle.icon"),
        )
        .tooltip("Agent Pin")
        .menu(&menu)
        .on_menu_event(handle_menu_event)
        .on_tray_icon_event(|tray, event| {
            handle_tray_icon_event(tray.app_handle(), event);
        })
        .build(app)?;

    // 注册 pins:changed 事件监听：Pin 状态变更后自动刷新托盘菜单。
    // Tauri 2 的 listen 返回 EventId（u32），监听器存活到应用退出，无需保持 guard。
    let app_for_listen = app.clone();
    app.listen("pins:changed", move |_| {
        refresh(&app_for_listen);
    });

    Ok(())
}

/// 处理托盘图标点击事件。
/// 左键单击/双击 → 打开管理界面（与右键菜单"打开管理界面"一致）。
/// 这是无主窗口托盘应用最核心的入口，用户本能地左键点托盘想恢复主界面。
fn handle_tray_icon_event(app: &AppHandle, event: TrayIconEvent) {
    match event {
        TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        }
        | TrayIconEvent::DoubleClick {
            button: MouseButton::Left,
            ..
        } => {
            if let Err(e) = open_manager_window(app) {
                eprintln!("[agent-pin] tray icon click open manager: {}", e);
            }
        }
        _ => {}
    }
}

/// 刷新托盘菜单（重建 + set_menu）。
///
/// 调用时机：
/// - 自动：registry emit "pins:changed" → tray listen → refresh（主路径）
/// - 手动：检查更新完成后（更新托盘 label 显示版本号），见 handle_check_update
///
/// 不再在此 emit 事件：事件由 registry 统一 emit，tray 是订阅者而非广播者。
pub fn refresh(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        eprintln!("[agent-pin] tray refresh: tray {} not found", TRAY_ID);
        return;
    };
    match build_menu(app) {
        Ok(menu) => {
            if let Err(e) = tray.set_menu(Some(menu)) {
                eprintln!("[agent-pin] tray refresh set_menu: {}", e);
            }
        }
        Err(e) => eprintln!("[agent-pin] tray refresh build_menu: {}", e),
    }
}

/// 构建托盘菜单。
/// 顺序：最近 5 hidden Pin（可空） → 管理界面 → 隐藏全部 → 分隔 → 退出
fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let menu = Menu::new(app)?;

    // 最近 5 hidden Pin（快恢入口）
    let recent = registry::REGISTRY.list_recent_hidden(RECENT_LIMIT);
    if !recent.is_empty() {
        for meta in recent {
            let title = truncate(&meta.title, TITLE_MAX_CHARS);
            let item = MenuItem::with_id(app, &meta.pin_id, title, true, None::<&str>)?;
            menu.append(&item)?;
        }
        menu.append(&PredefinedMenuItem::separator(app)?)?;
    }

    // 打开管理界面
    menu.append(&MenuItem::with_id(
        app,
        "open_manager",
        "打开管理界面",
        true,
        None::<&str>,
    )?)?;

    // 隐藏全部 Pin
    menu.append(&MenuItem::with_id(
        app,
        "hide_all",
        "隐藏全部 Pin",
        true,
        None::<&str>,
    )?)?;

    // 检查更新
    // 启动时已静默检查过一次（lib.rs setup），命中缓存时 here 读缓存显示版本号提示。
    // 缓存未命中或检查失败时显示通用"检查更新"文本。
    let update_label = build_update_label();
    menu.append(&MenuItem::with_id(
        app,
        MENU_CHECK_UPDATE,
        update_label,
        true,
        None::<&str>,
    )?)?;

    menu.append(&PredefinedMenuItem::separator(app)?)?;

    // 退出
    menu.append(&MenuItem::with_id(
        app,
        "quit",
        "退出 Agent Pin",
        true,
        None::<&str>,
    )?)?;

    Ok(menu)
}

/// 处理托盘菜单点击事件。
fn handle_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    let id = event.id.as_ref();
    match id {
        "quit" => app.exit(0),
        "open_manager" => {
            if let Err(e) = open_manager_window(app) {
                eprintln!("[agent-pin] open manager window: {}", e);
            }
        }
        "hide_all" => {
            // M10 修复：复用 pin_actions::hide_all_visible，消除三份拷贝
            // 不需要手动 refresh：hide_all_visible 内部调 set_state，
            // registry emit "pins:changed" → tray listen → 自动 refresh
            let failed = crate::pin_actions::hide_all_visible(app);
            if !failed.is_empty() {
                eprintln!(
                    "[agent-pin] tray hide_all failed for {} pin(s): {}",
                    failed.len(),
                    failed.join(", ")
                );
            }
        }
        MENU_CHECK_UPDATE => {
            handle_check_update(app);
        }
        _ => {
            // 假设是 pinId（最近 5 快恢入口）
            // 不需要手动 refresh：show_pin 内部调 set_state，
            // registry emit "pins:changed" → tray listen → 自动 refresh
            if id.starts_with("pin_") {
                if let Err(e) = crate::pin_actions::show_pin(app, id, ShowPinMode::Sync) {
                    eprintln!("[agent-pin] tray show pin {}: {}", id, e);
                }
            }
            // 未知 id：忽略，不 panic
        }
    }
}

/// 打开管理界面窗口（已存在则 show + 聚焦，不重建）。
/// pub 供 lib.rs setup 启动时调用。
/// 已存在时调 show()：窗口可能被用户点关闭按钮 hide 了（CloseRequested 拦截）。
pub fn open_manager_window(app: &AppHandle) -> tauri::Result<()> {
    const LABEL: &str = "manager";
    if let Some(existing) = app.get_webview_window(LABEL) {
        if let Err(e) = existing.show() {
            eprintln!("[agent-pin] manager show existing: {}", e);
        }
        if let Err(e) = existing.set_focus() {
            eprintln!("[agent-pin] manager focus existing: {}", e);
        }
        // Manager 重新可见时推送 snapshot：兜底 hidden 期间错过的状态变化
        // （Tauri 2 hidden 窗口事件投递不保证实时，show 时主动推一次确保一致）
        crate::manager_snapshot::emit_manager_snapshot(app);
        return Ok(());
    }
    WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html?manager=1".into()))
        .title("Agent Pin 管理")
        .inner_size(880.0, 620.0)
        .min_inner_size(640.0, 400.0)
        // 管理界面用系统装饰（不是 Pin 窗口，不需要自定义标题栏）
        .decorations(true)
        .resizable(true)
        .visible(true)
        .build()?;
    // 新建窗口：前端 mount 时会主动 invoke get_manager_snapshot，无需后端推
    Ok(())
}

/// 截断字符串，按 chars 处理中文。
fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars).collect();
        format!("{}…", truncated)
    }
}

/// 构建托盘"检查更新"菜单项的 label。
/// 若有 24h 内缓存的最新版本，显示版本号；否则显示通用"检查更新"。
fn build_update_label() -> String {
    match updater::cached_latest_version() {
        Some(latest) => format!("检查更新（最新 v{}）", latest),
        None => "检查更新".to_string(),
    }
}

/// 处理"检查更新"菜单点击。
/// spawn_blocking 调 updater::check(true)，根据结果弹 dialog 或打开浏览器。
/// M11 修复：用 AtomicBool 防止并发触发（连续点击只执行第一次，后续忽略）。
fn handle_check_update(app: &AppHandle) {
    // CAS 去重：若已有检查在进行中，直接返回
    if UPDATE_CHECK_IN_PROGRESS
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        eprintln!("[agent-pin] check_update already in progress, ignoring");
        return;
    }

    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        // 确保无论结果如何都重置标志位
        let result = tauri::async_runtime::spawn_blocking(|| updater::check(true)).await;
        UPDATE_CHECK_IN_PROGRESS.store(false, Ordering::SeqCst);

        match result {
            Ok(Ok(check)) => {
                if check.has_update {
                    // 有新版：弹 dialog 让用户选择是否打开浏览器
                    let app_for_dialog = app_handle.clone();
                    let url = check.release_url.clone();
                    let latest = check.latest_version.clone();
                    app_handle
                        .dialog()
                        .message(format!(
                            "发现新版本 v{}\n当前版本 v{}\n\n点击确定打开下载页面。",
                            latest, check.current_version
                        ))
                        .title("Agent Pin 有更新")
                        .show(move |ok_pressed| {
                            if ok_pressed {
                                // 用 std::process::Command 打开浏览器
                                if let Err(e) = open_release_url(&app_for_dialog, &url) {
                                    eprintln!("[agent-pin] open release url: {}", e);
                                }
                            }
                        });
                } else {
                    // 已是最新：弹 dialog 提示
                    app_handle
                        .dialog()
                        .message(format!("已是最新版本 v{}", check.current_version))
                        .title("Agent Pin")
                        .show(|_| {});
                }
                // 检查完成后刷新托盘（更新 label 显示版本号）
                refresh(&app_handle);
            }
            Ok(Err(e)) => {
                eprintln!("[agent-pin] check_update: {}", e);
                app_handle
                    .dialog()
                    .message("检查更新失败，请稍后重试或访问 GitHub Releases 页面。")
                    .title("Agent Pin")
                    .show(|_| {});
            }
            Err(e) => {
                eprintln!("[agent-pin] check_update join: {}", e);
            }
        }
    });
}

/// 打开 Release URL 到默认浏览器（跨平台）。
/// 不用 tauri-plugin-shell（已废弃 open 方法，推荐 tauri-plugin-opener），
/// 改用 std::process::Command，与 open_data_dir 一致风格，避免引入新插件。
/// Windows 用 explorer 而非 cmd /c start，避免 shell 元字符（&|>）被解析（M4 命令注入防护）。
fn open_release_url(_app: &AppHandle, url: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let cmd = "explorer";
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(target_os = "linux")]
    let cmd = "xdg-open";

    std::process::Command::new(cmd)
        .arg(url)
        .spawn()
        .map_err(|e| format!("open url: {}", e))?;
    Ok(())
}
