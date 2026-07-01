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
                    crate::registry::REGISTRY
                        .set_state(pin_id, PinState::Visible)
                        .map_err(ShowPinError::Internal)?;
                    // tray 刷新由 registry emit "pins:changed" → tray listen 自动处理
                }
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

    match mode {
        ShowPinMode::Sync => {
            // Sync 模式：cleanup + create + set_state(Visible) 同步执行。
            // 安全性不依赖"不 yield"——HTTP 调用方在 tokio worker 线程，主线程会并发处理
            // cleanup 触发的 Destroyed 事件。真正安全的原因：
            // 1. 正常路径（show hidden Pin）：state==Hidden，Destroyed handler 的
            //    `if meta.state == Visible` 守卫跳过，不会误设 Hidden。
            // 2. 重建路径（state==Visible 但窗口异常）：Destroyed handler 可能把 state
            //    设为 Hidden，但后续 set_state(Visible) 会覆盖回 Visible，最终一致。
            // 3. Destroyed handler 的 C1 守卫（get_webview_window 检查）在 create_pin_window
            //    成功后会跳过 state 更新（新窗口已存在）。
            if let Err(e) = crate::window::hide_pin_window(app, pin_id) {
                eprintln!("[agent-pin] show_pin cleanup for {}: {}", pin_id, e);
            }
            crate::window::create_pin_window(app, pin_id, &doc)
                .map_err(ShowPinError::WindowCreate)?;
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
        }
        ShowPinMode::AsyncCreate => {
            // C2 说明：此处先设 state=Visible 再异步创建窗口，存在短暂"state=Visible 但窗口不存在"
            // 的瞬态。这是有意为之：
            // 1. is_still_visible 依赖 state==Visible 判断是否应继续创建窗口，
            //    若 hide_pin 在窗口创建前被调用，state 变为 Hidden，异步任务会中止。
            // 2. 窗口创建失败时异步任务会把 state 回滚为 Hidden，瞬态自动纠正。
            // 3. C1 修复（Destroyed handler 校验 get_webview_window）已防止旧窗口的
            //    Destroyed 事件错误覆盖新窗口的 state。
            // 4. 不引入 Creating 中间态以避免 state 模型扩散到持久化/UI/托盘。
            crate::registry::REGISTRY
                .set_state(pin_id, PinState::Visible)
                .map_err(ShowPinError::Internal)?;
            // tray 刷新由 registry emit "pins:changed" → tray listen 自动处理

            let app = app.clone();
            let pin_id = pin_id.to_string();
            tauri::async_runtime::spawn(async move {
                if !is_still_visible(&pin_id) {
                    return;
                }

                // M-2 修复：cleanup 移入异步任务内部，在 set_state(Visible) 之后执行。
                // 原先 cleanup 在 set_state 之前调用，Destroyed 事件可能在 set_state 之后
                // 被处理，把 state 错误设为 Hidden。
                // 移入异步任务后，cleanup 触发的 Destroyed 事件即使在 create_pin_window 之前
                // 被处理，create_pin_window 成功后会重新 set_state(Visible) 覆盖误判。
                if let Err(e) = crate::window::hide_pin_window(&app, &pin_id) {
                    eprintln!("[agent-pin] show_pin cleanup for {}: {}", pin_id, e);
                }

                if let Err(e) = crate::window::create_pin_window(&app, &pin_id, &doc) {
                    if app.get_webview_window(&pin_id).is_some() && is_still_visible(&pin_id) {
                        // tray 刷新由 registry emit "pins:changed" → tray listen 自动处理
                        return;
                    }

                    eprintln!("[agent-pin] show_pin create window for {}: {}", pin_id, e);
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
                    let _ = app.emit(
                        "pin:show-failed",
                        serde_json::json!({ "pinId": pin_id, "message": e }),
                    );
                    return;
                }

                // 创建成功后重新 set_state(Visible)。
                // cleanup 触发的 Destroyed 事件可能把 state 误设为 Hidden，这里覆盖。
                // 守卫：只在窗口仍存在时才 re-affirm，避免覆盖用户并发 hide 的意图。
                // - Destroyed 误判（旧窗口销毁、新窗口存在）→ get_webview_window 返回 Some → re-affirm，正确。
                // - 用户 hide（新窗口被销毁）→ get_webview_window 返回 None → 跳过，保留 Hidden，正确。
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
