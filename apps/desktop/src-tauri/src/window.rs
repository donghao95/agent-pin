// Pin 窗口创建与管理
//
// 职责：
// - 为每个 Pin 创建独立的 Tauri WebviewWindow
// - 多 Pin 级联排列，避免完全重叠（右上角出生，向左下偏移 24px）
// - 无系统标题栏（decorations=false），title 融入内容首行，整窗可拖动
// - hide_pin_window：销毁窗口（幂等），用于 hide 路由和 show 路由清理孤儿窗口
// - fit_pin_window_height：前端渲染后测量内容高度，回流调整窗口高度（自适应）
//
// 窗口 URL 只携带 pinId，不携带完整 PinDocument。
// 前端通过 invoke(get_pin_document, pinId) 获取渲染数据。
//
// Phase 2-B：show 路由复用 create_pin_window（从 registry 读 doc 重建窗口）。
// 窗口位置不持久化：用户拖动后的位置丢失，show 时重新级联。
//
// 契约来源：docs/05_ui_style.md §4、docs/01_product_spec.md §9/§12

use tauri::{AppHandle, LogicalSize, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::pin::{PinDocument, PinHeight, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH};
use crate::storage::validate_pin_id;

/// 默认窗口宽度
const DEFAULT_WIDTH: f64 = 420.0;
/// 默认窗口高度（会在屏幕高度的 70% 内限制）
const DEFAULT_HEIGHT: f64 = 600.0;
/// height="auto" 时的初始保守高度。
/// 选 200px 而非 600px：避免内容很少时出现"大窗口→缩小"的视觉跳变。
/// 前端渲染后会通过 fit_pin_window_height 回流到实际内容高度。
const AUTO_INITIAL_HEIGHT: f64 = 200.0;
/// 级联偏移量（每个新窗口相对上一个偏移 24px）
const CASCADE_OFFSET: f64 = 24.0;
/// 右上角留白
const MARGIN: f64 = 40.0;
/// 窗口高度上限比例（屏幕高度的 70%）
const MAX_HEIGHT_RATIO: f64 = 0.7;
/// fit_pin_window_height 接收的 content_height 合理上限（与 validate 的 MAX_WINDOW_DIMENSION 对齐）
const FIT_HEIGHT_MAX: f64 = 100_000.0;

/// 为 Pin 创建独立桌面窗口。
/// 调用方需确保 label（pin_id）不冲突：show 路由应先调 hide_pin_window 清理孤儿窗口。
///
/// 尺寸优先级（高 → 低）：
/// 1. 用户记忆尺寸（PinMeta.window_size，用户手动 resize 后持久化）
/// 2. PinDocument.window 配置（Agent 通过 API/CLI 指定）
/// 3. 默认值（DEFAULT_WIDTH × DEFAULT_HEIGHT，或 auto 时 AUTO_INITIAL_HEIGHT）
///
/// 记忆尺寸优先的理由：用户手动调整后的尺寸是最贴近用户习惯的，应尊重。
/// 若用户未调整过（window_size=None），回退到 Agent 配置或默认值。
pub fn create_pin_window(app: &AppHandle, pin_id: &str, doc: &PinDocument) -> Result<(), String> {
    let label = pin_id.to_string();
    // URL 只带 pinId，数据走 invoke
    let url = format!("index.html?pinId={}", pin_id);

    let win_cfg = doc.window.as_ref();
    let always_on_top = win_cfg.and_then(|w| w.always_on_top).unwrap_or(true);

    // 从 registry 读取用户记忆尺寸（优先级最高）
    let remembered = crate::registry::REGISTRY
        .get_meta(pin_id)
        .and_then(|m| m.window_size);

    // width 优先级：记忆 > doc.window > 默认
    let width = remembered
        .as_ref()
        .map(|s| s.width)
        .or_else(|| win_cfg.and_then(|w| w.width).map(|w| w as f64))
        .unwrap_or(DEFAULT_WIDTH);

    // height 优先级：记忆 > doc.window > 默认
    // 记忆尺寸直接用（用户已确认过这个高度）；doc.window 的 auto 用保守初始高度
    let requested_height = remembered
        .as_ref()
        .map(|s| s.height)
        .or_else(|| {
            win_cfg.and_then(|w| w.height.as_ref()).map(|h| match h {
                PinHeight::Number(n) => *n as f64,
                PinHeight::Auto(_) => AUTO_INITIAL_HEIGHT,
            })
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
        // 最小尺寸约束：用户缩放时不会小于此值，保证内容可读性（见 docs/05_ui_style.md §4）。
        // 常量来自 packages/shared，与 validate 的最小值校验对齐（单一事实源），
        // 确保 API 层拒绝的值与窗口层强制的值一致，避免静默放大。
        .min_inner_size(MIN_WINDOW_WIDTH as f64, MIN_WINDOW_HEIGHT as f64)
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

/// 前端渲染后调用：根据内容实际高度调整 Pin 窗口高度（自适应）。
///
/// 流程：前端渲染完成 / 图片加载后测量 `.pin-body` 的 scrollHeight，
/// 通过 invoke 传 content_height 到后端，后端 clamp 后 set_size。
///
/// 记忆尺寸优先：若 PinMeta.window_size 存在（用户手动 resize 过），直接返回 Ok(false)，
/// 不覆盖用户选择的尺寸。这是"用户尺寸 > 自动适配"优先级的后端守卫，
/// 前端也通过 userResized 标志避免调用此函数，但后端守卫是单一事实源，
/// 确保即使前端逻辑有漏洞也不会覆盖用户记忆尺寸。
///
/// 校验：
/// - pin_id 格式（防路径遍历，复用 storage::validate_pin_id）
/// - content_height 在 [0, FIT_HEIGHT_MAX] 范围（防恶意传入超大值导致 set_size 异常）
/// - 窗口必须存在且是 Pin 窗口（label 以 pin_ 开头，防误操作 manager 窗口）
///
/// clamp 策略：
/// - 下限：MIN_WINDOW_HEIGHT（100，与 min_inner_size 对齐）
/// - 上限：屏幕高度 * 0.7（与 create_pin_window 的 MAX_HEIGHT_RATIO 一致）
/// - 无显示器信息时只应用下限，不 clamp 上限（极端环境兜底）
///
/// 返回 true 表示实际调整了高度，false 表示无需调整（有记忆尺寸、content_height 无效或窗口不存在）。
/// 错误返回 Err，不静默吞错（与 AGENTS.md 一致）。
pub fn fit_pin_window_height(
    app: &AppHandle,
    pin_id: &str,
    content_height: f64,
) -> Result<bool, String> {
    // 1. 校验 pin_id 格式（防路径遍历、防保留 label "manager"）
    validate_pin_id(pin_id)?;

    // 2. 校验 content_height 合理性
    if !content_height.is_finite() || content_height < 0.0 || content_height > FIT_HEIGHT_MAX {
        return Err(format!(
            "invalid content_height: {} (expected 0..={})",
            content_height, FIT_HEIGHT_MAX
        ));
    }

    // 3. 获取窗口，校验是 Pin 窗口（label 以 pin_ 开头）
    //    防止误调整 manager 窗口高度（manager 有自己的尺寸策略）
    let window = app
        .get_webview_window(pin_id)
        .ok_or_else(|| format!("window not found: {}", pin_id))?;
    if !pin_id.starts_with("pin_") {
        return Err(format!(
            "fit_pin_window_height can only be called on pin windows, got label: {}",
            pin_id
        ));
    }

    // 4. 记忆尺寸守卫：用户手动 resize 过的 Pin 不做自动适配。
    //    前端也通过 userResized 标志在调用前拦截，但后端是单一事实源，
    //    确保即使前端有竞态（remember_pin_size 尚未落盘时 measureAndFit 被触发）也不会覆盖用户尺寸。
    if let Some(meta) = crate::registry::REGISTRY.get_meta(pin_id) {
        if meta.window_size.is_some() {
            return Ok(false);
        }
    }

    // 5. clamp 高度
    let min_h = MIN_WINDOW_HEIGHT as f64;
    let target_height = match get_screen_size(app) {
        Some((_, screen_h)) => {
            let max_h = screen_h * MAX_HEIGHT_RATIO;
            content_height.max(min_h).min(max_h)
        }
        None => content_height.max(min_h), // 无显示器信息，只应用下限
    };

    // 6. 获取当前窗口宽度（保持宽度不变，只调高度）
    let current_size = window
        .inner_size()
        .map_err(|e| format!("failed to get inner_size: {}", e))?;
    let scale = window
        .scale_factor()
        .map_err(|e| format!("failed to get scale_factor: {}", e))?;
    let current_w = current_size.width as f64 / scale;

    // 7. set_size（LogicalSize：与 create_pin_window 的 inner_size 语义一致）
    window
        .set_size(LogicalSize::new(current_w, target_height))
        .map_err(|e| format!("failed to set_size: {}", e))?;

    Ok(true)
}
