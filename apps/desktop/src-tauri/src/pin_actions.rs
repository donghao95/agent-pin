use tauri::{AppHandle, Manager};

use crate::storage::PinState;

pub enum ShowPinMode {
    Sync,
    AsyncCreate,
}

pub fn show_pin(app: &AppHandle, pin_id: &str, mode: ShowPinMode) -> Result<(), String> {
    let meta = crate::registry::REGISTRY
        .get_meta(pin_id)
        .ok_or_else(|| format!("pin not found: {}", pin_id))?;

    if meta.state == PinState::Failed {
        return Err(format!("pin is failed: {}", pin_id));
    }

    if let Some(existing) = app.get_webview_window(pin_id) {
        match existing.show() {
            Ok(()) => {
                if let Err(e) = existing.set_focus() {
                    eprintln!("[agent-pin] show_pin focus {}: {}", pin_id, e);
                }
                crate::registry::REGISTRY.set_state(pin_id, PinState::Visible)?;
                crate::tray::refresh(app);
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

    if let Err(e) = crate::window::hide_pin_window(app, pin_id) {
        eprintln!("[agent-pin] show_pin cleanup for {}: {}", pin_id, e);
    }

    let doc = crate::registry::REGISTRY
        .get(pin_id)
        .ok_or_else(|| format!("pin doc missing: {}", pin_id))?;

    match mode {
        ShowPinMode::Sync => {
            crate::window::create_pin_window(app, pin_id, &doc)?;
            if let Err(e) = crate::registry::REGISTRY.set_state(pin_id, PinState::Visible) {
                if let Err(destroy_err) = crate::window::hide_pin_window(app, pin_id) {
                    eprintln!(
                        "[agent-pin] show_pin rollback destroy failed for {}: {}",
                        pin_id, destroy_err
                    );
                }
                return Err(e);
            }
            crate::tray::refresh(app);
        }
        ShowPinMode::AsyncCreate => {
            crate::registry::REGISTRY.set_state(pin_id, PinState::Visible)?;
            crate::tray::refresh(app);

            let app = app.clone();
            let pin_id = pin_id.to_string();
            tauri::async_runtime::spawn(async move {
                if !is_still_visible(&pin_id) {
                    return;
                }

                if let Err(e) = crate::window::create_pin_window(&app, &pin_id, &doc) {
                    if app.get_webview_window(&pin_id).is_some() && is_still_visible(&pin_id) {
                        crate::tray::refresh(&app);
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
                    crate::tray::refresh(&app);
                    return;
                }

                if !is_still_visible(&pin_id) {
                    if let Err(e) = crate::window::hide_pin_window(&app, &pin_id) {
                        eprintln!(
                            "[agent-pin] show_pin async cleanup after state changed for {}: {}",
                            pin_id, e
                        );
                    }
                    crate::tray::refresh(&app);
                    return;
                }

                crate::tray::refresh(&app);
            });
        }
    }

    Ok(())
}

fn is_still_visible(pin_id: &str) -> bool {
    crate::registry::REGISTRY
        .get_meta(pin_id)
        .map(|meta| meta.state == PinState::Visible)
        .unwrap_or(false)
}
