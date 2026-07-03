use tauri::{AppHandle, Emitter, Manager};

use crate::storage::PinState;

pub enum ShowPinMode {
    Sync,
    AsyncCreate,
}

/// show_pin 的类型化错误。
/// 替代原先的字符串前缀匹配（M1 修复），让调用方能可靠地映射到 HTTP 状态码。
#[derive(Debug)]
pub enum ShowPinError {
    /// Pin 不存在（meta 或 doc 缺失）
    NotFound(String),
    /// Pin 处于 failed 状态，不可显示
    Failed(String),
    /// 窗口创建失败
    WindowCreate(String),
    /// 持久化或其他内部错误（set_state 等）
    Internal(String),
}

impl std::fmt::Display for ShowPinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShowPinError::NotFound(msg) => write!(f, "pin not found: {}", msg),
            ShowPinError::Failed(msg) => write!(f, "pin is failed: {}", msg),
            ShowPinError::WindowCreate(msg) => write!(f, "failed to create pin window: {}", msg),
            ShowPinError::Internal(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for ShowPinError {}

pub fn show_pin(app: &AppHandle, pin_id: &str, mode: ShowPinMode) -> Result<(), ShowPinError> {
    let meta = crate::registry::REGISTRY
        .get_meta(pin_id)
        .ok_or_else(|| ShowPinError::NotFound(pin_id.to_string()))?;

    if meta.state == PinState::Failed {
        return Err(ShowPinError::Failed(pin_id.to_string()));
    }

    if let Some(existing) = app.get_webview_window(pin_id) {
        match existing.show() {
            Ok(()) => {
                if let Err(e) = existing.set_focus() {
                    eprintln!("[agent-pin] show_pin focus {}: {}", pin_id, e);
                }
                // 已 visible 且窗口 show 成功时跳过冗余的 set_state + refresh
                // （set_state 会写 state.json，无变更时不应触发 I/O）
                if meta.state != PinState::Visible {
                    // M6 修复：set_state 失败时窗口已 show，需 hide 窗口回滚，
                    // 避免"窗口可见但 state=hidden"的不一致。
                    if let Err(e) =
                        crate::registry::REGISTRY.set_state(pin_id, PinState::Visible)
                    {
                        if let Err(hide_err) = crate::window::hide_pin_window(app, pin_id) {
                            eprintln!(
                                "[agent-pin] show_pin rollback hide failed for {}: {}",
                                pin_id, hide_err
                            );
                        }
                        return Err(ShowPinError::Internal(e));
                    }
                    // tray 刷新由 registry emit "pins:changed" → tray listen 自动处理
                }
                // Manager 同步：推 snapshot（set_state 只 emit pins:changed 给 tray，不推 manager:snapshot）
                // AsyncCreate 模式下额外 emit pin:show-ready 让 Manager 退出 opening（M1 修复）
                if matches!(mode, ShowPinMode::AsyncCreate) {
                    if let Err(emit_err) = app.emit("pin:show-ready", pin_id) {
                        eprintln!("[agent-pin] emit pin:show-ready failed for {}: {}", pin_id, emit_err);
                    }
                }
                crate::manager_snapshot::emit_manager_snapshot(app);
                return Ok(());
            }
            Err(e) => {
                eprintln!(
                    "[agent-pin] show_pin existing window show failed for {}: {}",
                    pin_id, e
                );
                if let Err(destroy_err) = crate::window::hide_pin_window(app, pin_id) {
                    eprintln!(
                        "[agent-pin] show_pin existing window cleanup failed for {}: {}",
                        pin_id, destroy_err
                    );
                }
            }
        }
    } else if meta.state == PinState::Visible {
        eprintln!(
            "[agent-pin] show_pin state visible but window missing, recreating {}",
            pin_id
        );
    }

    let doc = crate::registry::REGISTRY
        .get(pin_id)
        .ok_or_else(|| ShowPinError::NotFound(format!("doc missing for {}", pin_id)))?;

    // 检测是否有旧窗口需要 cleanup。
    // 有旧窗口时，cleanup 会触发 Destroyed 事件，需要 mark_recreating 防止
    // Destroyed handler 误把新窗口的 state 设为 Hidden。
    // 旧方案用 get_webview_window(pin_id).is_some() 在 Destroyed 中判断，
    // 但 Tauri 2 Destroyed 触发时窗口可能未注销，返回 Some 误判（导致正常关闭也不刷新）。
    // 新方案用显式 recreating 标志，Destroyed 中检查 is_recreating 后 unmark。
    let had_window = app.get_webview_window(pin_id).is_some();
    if had_window {
        crate::registry::REGISTRY.mark_recreating(pin_id);
    }

    match mode {
        ShowPinMode::Sync => {
            // Sync 模式：cleanup + create + set_state(Visible) 同步执行。
            // 安全性：recreating 标志确保 cleanup 触发的 Destroyed 事件跳过 state 更新。
            // Destroyed 事件在 is_recreating=true 时 unmark + return，不覆盖 set_state(Visible)。
            if let Err(e) = crate::window::hide_pin_window(app, pin_id) {
                eprintln!("[agent-pin] show_pin cleanup for {}: {}", pin_id, e);
            }
            if let Err(e) = crate::window::create_pin_window(app, pin_id, &doc) {
                // m-2 修复：create 失败时 unmark_recreating，避免标志泄漏
                if had_window {
                    crate::registry::REGISTRY.unmark_recreating(pin_id);
                }
                return Err(ShowPinError::WindowCreate(e));
            }
            if let Err(e) = crate::registry::REGISTRY.set_state(pin_id, PinState::Visible) {
                if let Err(destroy_err) = crate::window::hide_pin_window(app, pin_id) {
                    eprintln!(
                        "[agent-pin] show_pin rollback destroy failed for {}: {}",
                        pin_id, destroy_err
                    );
                }
                return Err(ShowPinError::Internal(e));
            }
            // tray 刷新由 registry emit "pins:changed" → tray listen 自动处理
            // Manager 同步：推 snapshot
            crate::manager_snapshot::emit_manager_snapshot(app);
        }
        ShowPinMode::AsyncCreate => {
            // C2 说明：此处先设 state=Visible 再异步创建窗口，存在短暂"state=Visible 但窗口不存在"
            // 的瞬态。这是有意为之：
            // 1. is_still_visible 依赖 state==Visible 判断是否应继续创建窗口，
            //    若 hide_pin 在窗口创建前被调用，state 变为 Hidden，异步任务会中止。
            // 2. 窗口创建失败时异步任务会把 state 回滚为 Hidden，瞬态自动纠正。
            // 3. recreating 标志已防止旧窗口的 Destroyed 事件错误覆盖新窗口的 state。
            // 4. 不引入 Creating 中间态以避免 state 模型扩散到持久化/UI/托盘。
            // 5. Manager 前端用 opening 临时态覆盖此瞬态：set_state(Visible) 只 emit pins:changed
            //    （给 tray），不 emit manager:snapshot，所以 Manager 不会在窗口创建完成前显示 visible。
            //    创建成功后才 emit_manager_snapshot → Manager 退出 opening 显示 visible。
            // M5：set_state 失败时必须 unmark_recreating，否则标志泄漏。
            // 泄漏场景：set_state 失败 → show_pin 返回 Err → 旧窗口仍在 → 用户关闭旧窗口
            //   → Destroyed 触发 → is_recreating=true → 跳过 set_state(Hidden)
            //   → state 仍为旧值（可能 Visible），管理页不刷新。
            if let Err(e) = crate::registry::REGISTRY.set_state(pin_id, PinState::Visible) {
                if had_window {
                    crate::registry::REGISTRY.unmark_recreating(pin_id);
                }
                return Err(ShowPinError::Internal(e));
            }
            // tray 刷新由 registry emit "pins:changed" → tray listen 自动处理
            // 注意：此处不 emit manager:snapshot，Manager 保持 opening 临时态

            let app = app.clone();
            let pin_id = pin_id.to_string();
            tauri::async_runtime::spawn(async move {
                if !is_still_visible(&pin_id) {
                    // 异步任务开始前 state 已被并发 hide 改为 Hidden，
                    // 但 recreating 标志仍存在（mark 在 set_state 之前）。
                    // 必须清除，否则旧窗口（若存在）后续 Destroyed 会误判为重建跳过 state 更新。
                    crate::registry::REGISTRY.unmark_recreating(&pin_id);
                    // state 已是 Hidden（由 hide_pin 设置），推 snapshot + show-failed 让 Manager 退出 opening
                    if let Err(emit_err) = app.emit(
                        "pin:show-failed",
                        serde_json::json!({ "pinId": pin_id, "message": "cancelled by concurrent hide" }),
                    ) {
                        eprintln!("[agent-pin] emit pin:show-failed failed for {}: {}", pin_id, emit_err);
                    }
                    crate::manager_snapshot::emit_manager_snapshot(&app);
                    return;
                }

                // M-2 修复：cleanup 移入异步任务内部，在 set_state(Visible) 之后执行。
                // recreating 标志确保 cleanup 触发的 Destroyed 事件跳过 state 更新。
                if let Err(e) = crate::window::hide_pin_window(&app, &pin_id) {
                    eprintln!("[agent-pin] show_pin cleanup for {}: {}", pin_id, e);
                }

                if let Err(e) = crate::window::create_pin_window(&app, &pin_id, &doc) {
                    if app.get_webview_window(&pin_id).is_some() && is_still_visible(&pin_id) {
                        // 创建失败但窗口存在且仍应 visible：可能是并发创建成功，视为成功
                        // emit pin:show-ready 让 Manager 退出 opening（M1/M3 修复）
                        // m-2：unmark_recreating（幂等，若已由 Destroyed 清除则无操作）
                        if had_window {
                            crate::registry::REGISTRY.unmark_recreating(&pin_id);
                        }
                        if let Err(emit_err) = app.emit("pin:show-ready", &pin_id) {
                            eprintln!("[agent-pin] emit pin:show-ready failed for {}: {}", pin_id, emit_err);
                        }
                        crate::manager_snapshot::emit_manager_snapshot(&app);
                        return;
                    }

                    eprintln!("[agent-pin] show_pin create window for {}: {}", pin_id, e);
                    // m-2：create 失败时 unmark_recreating，避免标志泄漏
                    if had_window {
                        crate::registry::REGISTRY.unmark_recreating(&pin_id);
                    }
                    if let Err(state_err) =
                        crate::registry::REGISTRY.set_state(&pin_id, PinState::Hidden)
                    {
                        eprintln!(
                            "[agent-pin] show_pin rollback state for {}: {}",
                            pin_id, state_err
                        );
                    }
                    // m3：AsyncCreate 模式下 show_pin 已立即返回 Ok，
                    // 窗口异步创建失败时通过事件通知前端（Manager.tsx 监听）。
                    // tray 刷新由 set_state(Hidden) 触发 registry emit 自动处理
                    if let Err(emit_err) = app.emit(
                        "pin:show-failed",
                        serde_json::json!({ "pinId": pin_id, "message": e }),
                    ) {
                        eprintln!("[agent-pin] emit pin:show-failed failed for {}: {}", pin_id, emit_err);
                    }
                    // Manager 同步：推 snapshot（state=Hidden），让 Manager 退出 opening 显示 hidden
                    crate::manager_snapshot::emit_manager_snapshot(&app);
                    return;
                }

                // 创建成功后，必须再次检查 state 是否仍应 visible。
                // 并发场景：用户在窗口创建期间点了隐藏 → hide_pin 已 set_state(Hidden)。
                // 此时不能 re-affirm Visible，否则会覆盖用户的隐藏意图（C2 并发 bug 修复）。
                if !is_still_visible(&pin_id) {
                    // 用户已隐藏：销毁刚创建的窗口，state 已是 Hidden（由 hide_pin 设置）
                    let _ = crate::window::hide_pin_window(&app, &pin_id);
                    crate::registry::REGISTRY.unmark_recreating(&pin_id);
                    // 已隐藏：emit pin:show-failed 让 Manager 退出 opening（M3 修复）
                    if let Err(emit_err) = app.emit(
                        "pin:show-failed",
                        serde_json::json!({ "pinId": pin_id, "message": "cancelled by concurrent hide" }),
                    ) {
                        eprintln!("[agent-pin] emit pin:show-failed failed for {}: {}", pin_id, emit_err);
                    }
                    crate::manager_snapshot::emit_manager_snapshot(&app);
                    return;
                }

                // 仍应 visible：re-affirm set_state(Visible)。
                // cleanup 触发的 Destroyed 事件可能把 state 误设为 Hidden（若 recreating 未生效），
                // 这里覆盖。守卫：只在窗口仍存在时才 re-affirm，避免覆盖用户并发 hide 的意图。
                if app.get_webview_window(&pin_id).is_some() {
                    if let Err(e) = crate::registry::REGISTRY.set_state(&pin_id, PinState::Visible)
                    {
                        eprintln!(
                            "[agent-pin] show_pin re-affirm visible for {}: {}",
                            pin_id, e
                        );
                    }
                }
                // tray 刷新由 registry emit "pins:changed" → tray listen 自动处理
                // Manager 同步：推 snapshot（state=Visible）
                // emit pin:show-ready 让 Manager 退出 opening 临时态（M1/M2/M3/M4 修复）
                // 边界：manager:snapshot 只负责 setPins，pin:show-ready/show-failed 负责结束 opening
                if let Err(emit_err) = app.emit("pin:show-ready", &pin_id) {
                    eprintln!("[agent-pin] emit pin:show-ready failed for {}: {}", pin_id, emit_err);
                }
                crate::manager_snapshot::emit_manager_snapshot(&app);
            });
        }
    }

    Ok(())
}

/// 隐藏所有可见 Pin（共享逻辑，供 HTTP handler、Tauri command、托盘菜单复用）。
/// M10 修复：消除 tray.rs / lib.rs / http.rs 三份拷贝。
/// 行为：遍历所有 visible Pin，销毁窗口 + set_state_quiet(Hidden)，逐个记录失败但继续执行。
/// 返回值：失败的 pin_id 列表（空表示全部成功）。
///
/// 批量 emit 策略：循环内用 set_state_quiet 避免 N 次 emit 触发 N 次同步托盘菜单重建，
/// 循环结束后统一调用 emit_changed 一次，所有订阅者（托盘、管理界面）只刷新一次。
pub fn hide_all_visible(app: &AppHandle) -> Vec<String> {
    let metas = crate::registry::REGISTRY.list();
    let mut failed = Vec::new();
    for meta in metas {
        if meta.state != PinState::Visible {
            continue;
        }
        let pin_id = &meta.pin_id;
        // 销毁窗口（窗口不存在不算失败，hide_pin_window 幂等返回 Ok）
        if let Err(e) = crate::window::hide_pin_window(app, pin_id) {
            eprintln!("[agent-pin] hide_all window for {}: {}", pin_id, e);
            failed.push(pin_id.clone());
            continue;
        }
        // set_state_quiet：不 emit，避免循环内 N 次托盘重建
        if let Err(e) = crate::registry::REGISTRY.set_state_quiet(pin_id, PinState::Hidden) {
            eprintln!("[agent-pin] hide_all set_state for {}: {}", pin_id, e);
            failed.push(pin_id.clone());
        }
    }
    // 批量操作结束，统一 emit 一次，托盘和管理界面只刷新一次
    crate::registry::emit_changed();
    failed
}

fn is_still_visible(pin_id: &str) -> bool {
    crate::registry::REGISTRY
        .get_meta(pin_id)
        .map(|meta| meta.state == PinState::Visible)
        .unwrap_or(false)
}

// ---------- 测试 ----------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_show_pin_error_display() {
        // NotFound 包含 pin_id
        let e = ShowPinError::NotFound("pin_123".into());
        assert!(e.to_string().contains("pin not found"));
        assert!(e.to_string().contains("pin_123"));

        // Failed 包含 pin_id
        let e = ShowPinError::Failed("pin_456".into());
        assert!(e.to_string().contains("pin is failed"));
        assert!(e.to_string().contains("pin_456"));

        // WindowCreate 包含错误详情
        let e = ShowPinError::WindowCreate("webview init failed".into());
        assert!(e.to_string().contains("failed to create pin window"));
        assert!(e.to_string().contains("webview init failed"));

        // Internal 直接输出消息
        let e = ShowPinError::Internal("disk full".into());
        assert!(e.to_string().contains("disk full"));
    }

    #[test]
    fn test_show_pin_error_is_std_error() {
        // 确认实现了 std::error::Error trait
        fn assert_error<T: std::error::Error>() {}
        assert_error::<ShowPinError>();
    }
}
