// 系统托盘
//
// Phase 2-B 职责：
// 1. 最近 5 个 hidden Pin 快恢（点击即 show）
// 2. 打开管理界面（完整历史 + 搜索 + 删除）
// 3. 隐藏全部可见 Pin
// 4. 退出 Agent Pin
//
// 菜单动态刷新：Pin 状态变化时（show/hide/delete/create）调用方调 refresh()。
// Tauri 2 没提供"菜单即将显示时重建"的回调，所以必须主动 set_menu。
//
// 契约来源：docs/phase-plan.md Phase 2-B、docs/mvp-spec.md §13

use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager, WebviewUrl, WebviewWindowBuilder,
};

use crate::registry;
use crate::storage::PinState;

/// 托盘 id（用于 tray_by_id 获取后刷新菜单）
const TRAY_ID: &str = "main";
/// 托盘快恢列表最多显示多少个 hidden Pin
const RECENT_LIMIT: usize = 5;
/// 托盘菜单项标题最大字符数（中文按 chars 截断）
const TITLE_MAX_CHARS: usize = 30;

/// 构建系统托盘。在 Tauri setup hook中调用。
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
        .build(app)?;
    Ok(())
}

/// 刷新托盘菜单（重建 + set_menu）。
/// 在 Pin 状态变化后调用：create / show / hide / hide-all / delete。
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
            hide_all_visible(app);
            refresh(app);
        }
        _ => {
            // 假设是 pinId（最近 5 快恢入口）
            if id.starts_with("pin_") {
                if let Err(e) = show_pin_by_id(app, id) {
                    eprintln!("[agent-pin] tray show pin {}: {}", id, e);
                }
                refresh(app);
            }
            // 未知 id：忽略，不 panic
        }
    }
}

/// 隐藏所有 visible Pin（托盘"隐藏全部"用）。
fn hide_all_visible(app: &AppHandle) {
    let metas = registry::REGISTRY.list();
    for meta in metas {
        if meta.state != PinState::Visible {
            continue;
        }
        if let Err(e) = crate::window::hide_pin_window(app, &meta.pin_id) {
            eprintln!("[agent-pin] tray hide_all window {}: {}", meta.pin_id, e);
        }
        if let Err(e) = registry::REGISTRY.set_state(&meta.pin_id, PinState::Hidden) {
            eprintln!("[agent-pin] tray hide_all state {}: {}", meta.pin_id, e);
        }
    }
}

/// 按 pinId 显示 Pin（托盘快恢用）。
fn show_pin_by_id(app: &AppHandle, pin_id: &str) -> Result<(), String> {
    let doc = registry::REGISTRY
        .get(pin_id)
        .ok_or_else(|| format!("pin not found: {}", pin_id))?;
    // 清理可能的孤儿窗口（与 http.rs show_pin 和 lib.rs invoke show_pin 一致）
    if let Err(e) = crate::window::hide_pin_window(app, pin_id) {
        eprintln!("[agent-pin] tray show_pin_by_id cleanup for {}: {}", pin_id, e);
    }
    crate::window::create_pin_window(app, pin_id, &doc)?;
    // set_state 失败则回滚：destroy 刚创建的窗口，避免窗口可见但 state=hidden 的不一致
    if let Err(e) = registry::REGISTRY.set_state(pin_id, PinState::Visible) {
        if let Err(destroy_err) = crate::window::hide_pin_window(app, pin_id) {
            eprintln!(
                "[agent-pin] tray show_pin_by_id rollback for {}: {}",
                pin_id, destroy_err
            );
        }
        return Err(e);
    }
    Ok(())
}

/// 打开管理界面窗口（已存在则聚焦，不重建）。
fn open_manager_window(app: &AppHandle) -> tauri::Result<()> {
    const LABEL: &str = "manager";
    if let Some(existing) = app.get_webview_window(LABEL) {
        let _ = existing.set_focus();
        return Ok(());
    }
    WebviewWindowBuilder::new(
        app,
        LABEL,
        WebviewUrl::App("index.html?manager=1".into()),
    )
    .title("Agent Pin 管理")
    .inner_size(880.0, 620.0)
    .min_inner_size(640.0, 400.0)
    // 管理界面用系统装饰（不是 Pin 窗口，不需要自定义标题栏）
    .decorations(true)
    .resizable(true)
    .visible(true)
    .build()?;
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
