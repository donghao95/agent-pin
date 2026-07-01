// Pin 窗口创建与管理
//
// 职责：
// - 为每个 Pin 创建独立的 Tauri WebviewWindow
// - 多 Pin 级联排列，避免完全重叠（右上角出生，向左下偏移 24px）
// - 应用自定义轻标题栏（decorations=false，前端自己画标题栏）
// - hide_pin_window：销毁窗口（幂等），用于 hide 路由和 show 路由清理孤儿窗口
//
// 窗口 URL 只携带 pinId，不携带完整 PinDocument。
// 前端通过 invoke(get_pin_document, pinId) 获取渲染数据。
//
// Phase 2-B：show 路由复用 create_pin_window（从 registry 读 doc 重建窗口）。
// 窗口位置不持久化：用户拖动后的位置丢失，show 时重新级联。
//
// 契约来源：docs/05_ui_style.md §4、docs/01_product_spec.md §12

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::pin::{PinDocument, PinHeight};

/// 默认窗口宽度
const DEFAULT_WIDTH: f64 = 420.0;
/// 默认窗口高度（会在屏幕高度的 70% 内限制）
const DEFAULT_HEIGHT: f64 = 600.0;
/// 级联偏移量（每个新窗口相对上一个偏移 24px）
const CASCADE_OFFSET: f64 = 24.0;
/// 右上角留白
const MARGIN: f64 = 40.0;
/// 窗口高度上限比例（屏幕高度的 70%）
const MAX_HEIGHT_RATIO: f64 = 0.7;

/// 为 Pin 创建独立桌面窗口。
/// 调用方需确保 label（pin_id）不冲突：show 路由应先调 hide_pin_window 清理孤儿窗口。
pub fn create_pin_window(app: &AppHandle, pin_id: &str, doc: &PinDocument) -> Result<(), String> {
    let label = pin_id.to_string();
    // URL 只带 pinId，数据走 invoke
    let url = format!("index.html?pinId={}", pin_id);

    let win_cfg = doc.window.as_ref();
    let width = win_cfg
        .and_then(|w| w.width)
        .map(|w| w as f64)
        .unwrap_or(DEFAULT_WIDTH);
    let always_on_top = win_cfg.and_then(|w| w.always_on_top).unwrap_or(true);

    // height：数值直接用，"auto" 或未指定用 DEFAULT_HEIGHT。
    let requested_height = win_cfg
        .and_then(|w| w.height.as_ref())
        .map(|h| match h {
            PinHeight::Number(n) => *n as f64,
            PinHeight::Auto(_) => DEFAULT_HEIGHT,
        })
        .unwrap_or(DEFAULT_HEIGHT);

    // M8 修复：在 build 之前 clamp 高度，避免 build 后 set_size 产生闪烁。
    // M9 修复：primary_monitor 统一用 get_screen_size 辅助函数处理 None。
    let height = match get_screen_size(app) {
        Some((_, screen_h)) => {
            let max_h = screen_h * MAX_HEIGHT_RATIO;
            requested_height.min(max_h)
        }
        None => requested_height, // 无显示器信息，不 clamp（极端环境兜底）
    };

    // 位置：如果请求体指定了 x,y 就用，否则级联
    let (x, y) = if let (Some(x), Some(y)) = (win_cfg.and_then(|w| w.x), win_cfg.and_then(|w| w.y))
    {
        (x as f64, y as f64)
    } else {
        compute_cascade_position(app, width)?
    };

    let _window = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(url.into()))
        .title(&doc.title)
        .inner_size(width, height)
        .position(x, y)
        // 自定义轻标题栏：去掉系统装饰
        .decorations(false)
        // Windows: shadow(true) 让 DWM 给无边框窗口加 OS 级阴影 + Win11 圆角，
        // 解决移动/缩放时 WebView2 重绘延迟导致的黑线。
        // CSS 不再做圆角/阴影（见 pin.css），统一由 DWM 提供 OS 级圆角。
        // 平台差异：Win11 有圆角+阴影；Win10 退化为直角；macOS/Linux 行为由系统合成器决定，Phase 1 不验证。
        // 残留问题：移动/缩放时 WebView2 重绘延迟仍可能边缘闪烁，Phase 1 接受（见 docs/05_ui_style.md §4）。
        .shadow(true)
        .always_on_top(always_on_top)
        .resizable(true)
        .skip_taskbar(true)
        .visible(true)
        .build()
        .map_err(|e| format!("failed to create pin window: {}", e))?;

    Ok(())
}

/// 隐藏 Pin 窗口（destroy）。
/// 幂等：窗口不存在返回 Ok。
/// 注意：destroy 会触发 Destroyed 事件，lib.rs 的 on_window_event 负责状态更新。
pub fn hide_pin_window(app: &AppHandle, pin_id: &str) -> Result<(), String> {
    if let Some(win) = app.get_webview_window(pin_id) {
        win.destroy()
            .map_err(|e| format!("failed to destroy window {}: {}", pin_id, e))?;
    }
    // 窗口不存在：幂等返回 Ok
    Ok(())
}

/// 计算级联位置：右上角出生，向左下偏移。
fn compute_cascade_position(app: &AppHandle, win_w: f64) -> Result<(f64, f64), String> {
    // M9 修复：primary_monitor None 时返回 Err（与无显示器环境一致，不静默兜底）
    let (screen_w, screen_h) =
        get_screen_size(app).ok_or_else(|| "no primary monitor".to_string())?;

    // M7 修复：只统计当前可见的 Pin 窗口，排除正在销毁/已隐藏的窗口。
    // 原先 count 包含所有 webview_windows（含正在 destroy 的），导致偏移过大。
    let count = app
        .webview_windows()
        .values()
        .filter(|w| w.label() != "manager")
        .filter(|w| w.is_visible().unwrap_or(false))
        .count();
    let offset = (count as f64) * CASCADE_OFFSET;

    // 右上角，向左下偏移；clamp 防止超出屏幕
    let x = (screen_w - win_w - MARGIN - offset).max(10.0);
    let y = (MARGIN + offset).min(screen_h * 0.7);

    Ok((x, y))
}

/// 获取主显示器的逻辑尺寸（宽, 高）。
/// M9 修复：统一 primary_monitor 的 None 处理，避免 create_pin_window 和 compute_cascade_position 不一致。
/// 返回 None 表示无显示器信息（headless 等极端环境）。
fn get_screen_size(app: &AppHandle) -> Option<(f64, f64)> {
    let monitor = app.primary_monitor().ok().flatten()?;
    let scale = monitor.scale_factor();
    let screen_w = monitor.size().width as f64 / scale;
    let screen_h = monitor.size().height as f64 / scale;
    Some((screen_w, screen_h))
}
