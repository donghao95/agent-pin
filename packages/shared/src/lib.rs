// Agent Pin 共享类型与校验逻辑
//
// 这是 API / CLI / 窗口渲染之间的核心契约。
// desktop 后端和 agent-pin CLI 共用此 crate，避免类型漂移。
//
// Phase 2 起接受 markdown / image / status block，支持多 block 混排。
//
// 契约来源：docs/01_product_spec.md §7/§8、docs/03_api.md §9

use std::path::Path;

use serde::{Deserialize, Serialize};

// ---------- PinDocument ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinDocument {
    pub version: u32,
    pub title: String,
    pub blocks: Vec<PinBlock>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub window: Option<PinWindowConfig>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub source: Option<PinSource>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub created_at: Option<String>,
}

// ---------- Block ----------

/// PinBlock 使用 internally tagged enum（tag = "type"）。
/// JSON 形如 {"type":"markdown","content":"..."}。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PinBlock {
    Markdown(MarkdownBlock),
    Image(ImageBlock),
    Status(StatusBlock),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkdownBlock {
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageBlock {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub caption: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusBlock {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub level: Option<String>,
    pub text: String,
}

// ---------- Window / Source ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinWindowConfig {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub width: Option<u32>,
    /// height 可以是数字或字符串 "auto"
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub height: Option<PinHeight>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub x: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub y: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub always_on_top: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PinHeight {
    Number(u32),
    Auto(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinSource {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub workspace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub task: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub conversation_id: Option<String>,
}

// ---------- 错误码 ----------

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
// 所有变体均预留给 desktop 后端 / HTTP 层 / 未来扩展，validate() 只返回其中一部分。
// 此处统一 allow(dead_code) 避免未使用变体告警。
#[allow(dead_code)]
pub enum PinErrorCode {
    InvalidJson,
    InvalidPinDocument,
    /// 未知 block type。注意：serde internally tagged enum 在反序列化阶段就拒绝未知 type，
    /// 错误被 http.rs 映射为 InvalidJson，此码当前不会由 validate() 返回。
    /// 保留是为了文档契约和未来可能的兜底。
    UnsupportedBlockType,
    /// 图片文件不存在。HTTP 层不校验文件存在性（desktop 不知道 Agent cwd），
    /// 此码当前不会由 validate() 返回；前端 <img> onError 显示错误块。
    ImageNotFound,
    /// 图片格式不支持（非 PNG/JPG/JPEG/WebP/GIF）。
    ImageUnsupported,
    /// show/hide/delete 路由中 pinId 不存在。
    PinNotFound,
    WindowCreateFailed,
    InternalError,
}

/// 长度上限（防失控 Agent 推送超大内容导致前端卡顿或内存压力）。
/// title 上限 1024 字符（按 chars 计，中文友好）。
pub const MAX_TITLE_CHARS: usize = 1024;
/// Markdown content 上限 256KB（字节）。
pub const MAX_CONTENT_BYTES: usize = 256 * 1024;
/// blocks 数量上限。
pub const MAX_BLOCKS: usize = 50;
/// image caption 上限 1024 字符（与 title 一致，按 chars 计）。
pub const MAX_CAPTION_CHARS: usize = 1024;
/// status text 上限 4096 字符（比 title 长，但仍有限防膨胀）。
pub const MAX_STATUS_TEXT_CHARS: usize = 4096;
/// source 各字段上限 256 字符。
pub const MAX_SOURCE_FIELD_CHARS: usize = 256;
/// image path 长度上限（字节）。防超长 path 导致持久化膨胀和前端渲染问题。
pub const MAX_IMAGE_PATH_BYTES: usize = 4096;
/// 窗口宽度数值下限（与 window.rs 的 min_inner_size 对齐，见 docs/05_ui_style.md §4）。
/// 小于此值的 width 会被 validate 拒绝，而不是静默放大到最小宽度。
pub const MIN_WINDOW_WIDTH: u32 = 280;
/// 窗口高度数值下限（与 window.rs 的 min_inner_size 对齐，保证标题栏可见）。
/// 小于此值的 height 会被 validate 拒绝。
pub const MIN_WINDOW_HEIGHT: u32 = 100;
/// 窗口宽度/高度数值上限（防超出屏幕导致创建失败）。
pub const MAX_WINDOW_DIMENSION: u32 = 100_000;
/// 窗口 x/y 坐标范围（防极值导致窗口创建 panic）。
pub const MIN_WINDOW_POS: i32 = -100_000;
pub const MAX_WINDOW_POS: i32 = 100_000;

/// 支持的图片扩展名白名单（docs/01_product_spec.md §8）。
/// 同时用于 HTTP 校验和前端防御，避免 asset protocol 加载非图片文件。
/// pub 以便 CLI 未来提前校验扩展名，避免 desktop 拒绝后才报错。
pub const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif"];

#[derive(Debug, Clone, Serialize)]
pub struct PinError {
    pub code: PinErrorCode,
    pub message: String,
}

impl PinError {
    pub fn new(code: PinErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

// ---------- 校验 ----------

/// 检查字符串是否包含不允许的控制字符。
/// 允许 \n \t \r（正常文本换行/制表符），拒绝其他 C0 控制符和 null 字节。
/// 防止 null 字节注入底层 C API（如 Tauri asset protocol 文件路径截断）。
fn has_disallowed_control_chars(s: &str) -> bool {
    s.chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t' && c != '\r')
}

/// 校验 PinDocument。
/// Phase 2 起接受 markdown / image / status block，支持多 block 混排。
/// 注意：HTTP 层不校验 image path 文件存在性（路径相对于 Agent cwd，desktop 不知道）。
/// 图片不存在的错误在前端渲染时通过 <img> onerror 显示错误块。
pub fn validate(doc: &PinDocument) -> Result<(), PinError> {
    if doc.version != 1 {
        return Err(PinError::new(
            PinErrorCode::InvalidPinDocument,
            "version must be 1",
        ));
    }
    // 控制字符校验：拒绝 null 字节和其他 C0 控制符，防止底层 API 截断和渲染异常。
    // 允许 \n \t \r（正常 Markdown 文本需要）。
    if has_disallowed_control_chars(&doc.title) {
        return Err(PinError::new(
            PinErrorCode::InvalidPinDocument,
            "title must not contain control characters",
        ));
    }
    if doc.title.trim().is_empty() {
        return Err(PinError::new(
            PinErrorCode::InvalidPinDocument,
            "title must be non-empty",
        ));
    }
    // title 长度上限（按 chars 计，中文友好）
    if doc.title.chars().count() > MAX_TITLE_CHARS {
        return Err(PinError::new(
            PinErrorCode::InvalidPinDocument,
            format!("title too long (max {} chars)", MAX_TITLE_CHARS),
        ));
    }
    // source 各字段长度校验（防持久化文件膨胀和前端列表异常）
    if let Some(src) = &doc.source {
        for (name, val) in [
            ("agent", src.agent.as_deref()),
            ("workspace", src.workspace.as_deref()),
            ("task", src.task.as_deref()),
            ("conversationId", src.conversation_id.as_deref()),
        ] {
            if let Some(s) = val {
                if s.chars().count() > MAX_SOURCE_FIELD_CHARS {
                    return Err(PinError::new(
                        PinErrorCode::InvalidPinDocument,
                        format!(
                            "source.{} too long (max {} chars)",
                            name, MAX_SOURCE_FIELD_CHARS
                        ),
                    ));
                }
            }
        }
    }
    // 校验 window 数值范围：
    // - width 必须在 [MIN_WINDOW_WIDTH, MAX_WINDOW_DIMENSION] 内（与窗口 min_inner_size 对齐，拒绝静默放大）
    // - height(Number) 必须在 [MIN_WINDOW_HEIGHT, MAX_WINDOW_DIMENSION] 内（保证标题栏可见）
    // 防 0 或极小值导致不可见窗口，防超大值导致创建失败（M11）。
    if let Some(win) = &doc.window {
        if let Some(w) = win.width {
            if !(MIN_WINDOW_WIDTH..=MAX_WINDOW_DIMENSION).contains(&w) {
                return Err(PinError::new(
                    PinErrorCode::InvalidPinDocument,
                    format!(
                        "window.width must be in [{}..{}], got {}",
                        MIN_WINDOW_WIDTH, MAX_WINDOW_DIMENSION, w
                    ),
                ));
            }
        }
        // 校验 window.height：如果是字符串变体，值必须恰好是 "auto"。
        // PinHeight 用 untagged 反序列化，任何字符串都会被当作 Auto，需要在这里拦截非法值，
        // 避免 "tall"/"100px" 等错误输入被静默当作 auto 处理。
        // 注意：字符串数字如 "100" 也会被 untagged 当作 Auto，validate 会拒绝并提示
        // "must be \"auto\""。这是 untagged 的固有行为，错误信息已尽量清晰。
        if let Some(PinHeight::Auto(s)) = &win.height {
            if s != "auto" {
                // 如果是字符串数字（如 "100"），给出更明确的提示
                let hint = if s.parse::<u32>().is_ok() {
                    format!(
                        "; if you mean height {}, use JSON number {} instead of string \"{}\"",
                        s, s, s
                    )
                } else {
                    String::new()
                };
                return Err(PinError::new(
                    PinErrorCode::InvalidPinDocument,
                    format!("window.height string must be \"auto\", got {:?}{}", s, hint),
                ));
            }
        }
        if let Some(PinHeight::Number(n)) = &win.height {
            if !(MIN_WINDOW_HEIGHT..=MAX_WINDOW_DIMENSION).contains(n) {
                return Err(PinError::new(
                    PinErrorCode::InvalidPinDocument,
                    format!(
                        "window.height must be in [{}..{}], got {}",
                        MIN_WINDOW_HEIGHT, MAX_WINDOW_DIMENSION, n
                    ),
                ));
            }
        }
        if let Some(x) = win.x {
            if !(MIN_WINDOW_POS..=MAX_WINDOW_POS).contains(&x) {
                return Err(PinError::new(
                    PinErrorCode::InvalidPinDocument,
                    format!(
                        "window.x must be in [{}..{}], got {}",
                        MIN_WINDOW_POS, MAX_WINDOW_POS, x
                    ),
                ));
            }
        }
        if let Some(y) = win.y {
            if !(MIN_WINDOW_POS..=MAX_WINDOW_POS).contains(&y) {
                return Err(PinError::new(
                    PinErrorCode::InvalidPinDocument,
                    format!(
                        "window.y must be in [{}..{}], got {}",
                        MIN_WINDOW_POS, MAX_WINDOW_POS, y
                    ),
                ));
            }
        }
    }
    if doc.blocks.is_empty() {
        return Err(PinError::new(
            PinErrorCode::InvalidPinDocument,
            "blocks must contain at least one block",
        ));
    }
    if doc.blocks.len() > MAX_BLOCKS {
        return Err(PinError::new(
            PinErrorCode::InvalidPinDocument,
            format!("too many blocks (max {})", MAX_BLOCKS),
        ));
    }
    for (i, block) in doc.blocks.iter().enumerate() {
        match block {
            PinBlock::Markdown(m) => {
                if m.content.trim().is_empty() {
                    return Err(PinError::new(
                        PinErrorCode::InvalidPinDocument,
                        format!("blocks[{}].content must be non-empty", i),
                    ));
                }
                if has_disallowed_control_chars(&m.content) {
                    return Err(PinError::new(
                        PinErrorCode::InvalidPinDocument,
                        format!("blocks[{}].content must not contain control characters", i),
                    ));
                }
                if m.content.len() > MAX_CONTENT_BYTES {
                    return Err(PinError::new(
                        PinErrorCode::InvalidPinDocument,
                        format!(
                            "blocks[{}].content too large (max {} bytes)",
                            i, MAX_CONTENT_BYTES
                        ),
                    ));
                }
            }
            PinBlock::Image(img) => {
                if img.path.trim().is_empty() {
                    return Err(PinError::new(
                        PinErrorCode::InvalidPinDocument,
                        format!("blocks[{}].path must be non-empty", i),
                    ));
                }
                if has_disallowed_control_chars(&img.path) {
                    return Err(PinError::new(
                        PinErrorCode::InvalidPinDocument,
                        format!("blocks[{}].path must not contain control characters", i),
                    ));
                }
                if img.path.len() > MAX_IMAGE_PATH_BYTES {
                    return Err(PinError::new(
                        PinErrorCode::InvalidPinDocument,
                        format!(
                            "blocks[{}].path too long (max {} bytes)",
                            i, MAX_IMAGE_PATH_BYTES
                        ),
                    ));
                }
                // caption 校验：控制字符 + 长度
                if let Some(cap) = &img.caption {
                    if has_disallowed_control_chars(cap) {
                        return Err(PinError::new(
                            PinErrorCode::InvalidPinDocument,
                            format!("blocks[{}].caption must not contain control characters", i),
                        ));
                    }
                    if cap.chars().count() > MAX_CAPTION_CHARS {
                        return Err(PinError::new(
                            PinErrorCode::InvalidPinDocument,
                            format!(
                                "blocks[{}].caption too long (max {} chars)",
                                i, MAX_CAPTION_CHARS
                            ),
                        ));
                    }
                }
                // path 必须是绝对路径（docs/01_product_spec.md §8）。
                // 不校验文件存在性：desktop 不知道 Agent cwd，前端 <img> onerror 处理。
                if !Path::new(&img.path).is_absolute() {
                    return Err(PinError::new(
                        PinErrorCode::InvalidPinDocument,
                        format!("blocks[{}].path must be absolute (relative path resolution is CLI's job)", i),
                    ));
                }
                // 扩展名白名单校验（docs/01_product_spec.md §8：PNG/JPG/JPEG/WebP/GIF）。
                // 同时避免 asset protocol 加载非图片文件（安全缓解）。
                let ext = Path::new(&img.path)
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_ascii_lowercase())
                    .unwrap_or_default();
                if !IMAGE_EXTENSIONS.contains(&ext.as_str()) {
                    return Err(PinError::new(
                        PinErrorCode::ImageUnsupported,
                        format!(
                            "blocks[{}].path has unsupported image format (supported: {:?})",
                            i, IMAGE_EXTENSIONS
                        ),
                    ));
                }
            }
            PinBlock::Status(s) => {
                if s.text.trim().is_empty() {
                    return Err(PinError::new(
                        PinErrorCode::InvalidPinDocument,
                        format!("blocks[{}].text must be non-empty", i),
                    ));
                }
                if has_disallowed_control_chars(&s.text) {
                    return Err(PinError::new(
                        PinErrorCode::InvalidPinDocument,
                        format!("blocks[{}].text must not contain control characters", i),
                    ));
                }
                // text 长度校验（防前端卡顿）
                if s.text.chars().count() > MAX_STATUS_TEXT_CHARS {
                    return Err(PinError::new(
                        PinErrorCode::InvalidPinDocument,
                        format!(
                            "blocks[{}].text too long (max {} chars)",
                            i, MAX_STATUS_TEXT_CHARS
                        ),
                    ));
                }
                if let Some(level) = &s.level {
                    if !matches!(level.as_str(), "info" | "success" | "warning" | "error") {
                        return Err(PinError::new(
                            PinErrorCode::InvalidPinDocument,
                            format!(
                                "blocks[{}].level must be one of info/success/warning/error",
                                i
                            ),
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

// ---------- 测试 ----------
//
// 覆盖 validate() 的主要边界：版本、title、blocks、window 数值范围、image 路径/扩展名、
// status level 白名单、长度上限。
// PinHeight untagged 行为通过反序列化测试验证。

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_doc() -> PinDocument {
        PinDocument {
            version: 1,
            title: "test".to_string(),
            blocks: vec![PinBlock::Markdown(MarkdownBlock {
                content: "hello".to_string(),
            })],
            window: None,
            source: None,
            created_at: None,
        }
    }

    #[test]
    fn test_valid_markdown() {
        assert!(validate(&valid_doc()).is_ok());
    }

    #[test]
    fn test_valid_image_absolute() {
        let mut doc = valid_doc();
        // 跨平台绝对路径：Windows 需要 drive 前缀，Unix 用 /
        let abs_path = if cfg!(windows) {
            "C:\\home\\user\\img.png"
        } else {
            "/home/user/img.png"
        };
        doc.blocks = vec![PinBlock::Image(ImageBlock {
            path: abs_path.to_string(),
            caption: None,
        })];
        assert!(validate(&doc).is_ok());
    }

    #[test]
    fn test_valid_status_all_levels() {
        for level in &["info", "success", "warning", "error"] {
            let mut doc = valid_doc();
            doc.blocks = vec![PinBlock::Status(StatusBlock {
                level: Some(level.to_string()),
                text: "ok".to_string(),
            })];
            assert!(validate(&doc).is_ok(), "level {} should be valid", level);
        }
    }

    #[test]
    fn test_invalid_version() {
        let mut doc = valid_doc();
        doc.version = 2;
        assert!(validate(&doc).is_err());
    }

    #[test]
    fn test_empty_title() {
        let mut doc = valid_doc();
        doc.title = "   ".to_string();
        assert!(validate(&doc).is_err());
    }

    #[test]
    fn test_title_too_long() {
        let mut doc = valid_doc();
        doc.title = "a".repeat(MAX_TITLE_CHARS + 1);
        assert!(validate(&doc).is_err());
    }

    #[test]
    fn test_empty_blocks() {
        let mut doc = valid_doc();
        doc.blocks = vec![];
        assert!(validate(&doc).is_err());
    }

    #[test]
    fn test_too_many_blocks() {
        let mut doc = valid_doc();
        doc.blocks = (0..MAX_BLOCKS + 1)
            .map(|_| {
                PinBlock::Markdown(MarkdownBlock {
                    content: "x".to_string(),
                })
            })
            .collect();
        assert!(validate(&doc).is_err());
    }

    #[test]
    fn test_markdown_content_too_large() {
        let mut doc = valid_doc();
        doc.blocks = vec![PinBlock::Markdown(MarkdownBlock {
            content: "a".repeat(MAX_CONTENT_BYTES + 1),
        })];
        assert!(validate(&doc).is_err());
    }

    #[test]
    fn test_window_width_zero() {
        let mut doc = valid_doc();
        doc.window = Some(PinWindowConfig {
            width: Some(0),
            height: None,
            x: None,
            y: None,
            always_on_top: None,
        });
        assert!(validate(&doc).is_err());
    }

    #[test]
    fn test_window_height_number_zero() {
        let mut doc = valid_doc();
        doc.window = Some(PinWindowConfig {
            width: None,
            height: Some(PinHeight::Number(0)),
            x: None,
            y: None,
            always_on_top: None,
        });
        assert!(validate(&doc).is_err());
    }

    // ---------- window width/height 最小值边界测试 ----------
    // validate 的最小值与 window.rs 的 min_inner_size 对齐（280/100），
    // 小于此值的请求会被拒绝而非静默放大（避免 Agent 困惑）。

    #[test]
    fn test_window_width_below_minimum_rejected() {
        let mut doc = valid_doc();
        doc.window = Some(PinWindowConfig {
            width: Some(MIN_WINDOW_WIDTH - 1),
            height: None,
            x: None,
            y: None,
            always_on_top: None,
        });
        assert!(validate(&doc).is_err());
    }

    #[test]
    fn test_window_width_at_minimum_accepted() {
        let mut doc = valid_doc();
        doc.window = Some(PinWindowConfig {
            width: Some(MIN_WINDOW_WIDTH),
            height: None,
            x: None,
            y: None,
            always_on_top: None,
        });
        assert!(validate(&doc).is_ok());
    }

    #[test]
    fn test_window_height_below_minimum_rejected() {
        let mut doc = valid_doc();
        doc.window = Some(PinWindowConfig {
            width: None,
            height: Some(PinHeight::Number(MIN_WINDOW_HEIGHT - 1)),
            x: None,
            y: None,
            always_on_top: None,
        });
        assert!(validate(&doc).is_err());
    }

    #[test]
    fn test_window_height_at_minimum_accepted() {
        let mut doc = valid_doc();
        doc.window = Some(PinWindowConfig {
            width: None,
            height: Some(PinHeight::Number(MIN_WINDOW_HEIGHT)),
            x: None,
            y: None,
            always_on_top: None,
        });
        assert!(validate(&doc).is_ok());
    }

    #[test]
    fn test_window_height_auto() {
        let mut doc = valid_doc();
        doc.window = Some(PinWindowConfig {
            width: None,
            height: Some(PinHeight::Auto("auto".to_string())),
            x: None,
            y: None,
            always_on_top: None,
        });
        assert!(validate(&doc).is_ok());
    }

    #[test]
    fn test_window_height_auto_invalid_string() {
        let mut doc = valid_doc();
        doc.window = Some(PinWindowConfig {
            width: None,
            height: Some(PinHeight::Auto("tall".to_string())),
            x: None,
            y: None,
            always_on_top: None,
        });
        assert!(validate(&doc).is_err());
    }

    #[test]
    fn test_image_relative_path_rejected() {
        let mut doc = valid_doc();
        doc.blocks = vec![PinBlock::Image(ImageBlock {
            path: "relative/img.png".to_string(),
            caption: None,
        })];
        assert!(validate(&doc).is_err());
    }

    #[test]
    fn test_image_unsupported_extension() {
        let mut doc = valid_doc();
        let abs_path = if cfg!(windows) {
            "C:\\abs\\img.bmp"
        } else {
            "/abs/img.bmp"
        };
        doc.blocks = vec![PinBlock::Image(ImageBlock {
            path: abs_path.to_string(),
            caption: None,
        })];
        let err = validate(&doc).unwrap_err();
        assert_eq!(err.code, PinErrorCode::ImageUnsupported);
    }

    #[test]
    fn test_image_extension_case_insensitive() {
        let mut doc = valid_doc();
        let abs_path = if cfg!(windows) {
            "C:\\abs\\IMG.PNG"
        } else {
            "/abs/IMG.PNG"
        };
        doc.blocks = vec![PinBlock::Image(ImageBlock {
            path: abs_path.to_string(),
            caption: None,
        })];
        assert!(
            validate(&doc).is_ok(),
            "uppercase extension should be valid"
        );
    }

    #[test]
    fn test_status_invalid_level() {
        let mut doc = valid_doc();
        doc.blocks = vec![PinBlock::Status(StatusBlock {
            level: Some("critical".to_string()),
            text: "ok".to_string(),
        })];
        assert!(validate(&doc).is_err());
    }

    // ---------- PinHeight untagged 反序列化行为测试 ----------

    #[test]
    fn test_pin_height_number_deserialize() {
        let json = r#"{"height": 100}"#;
        let cfg: PinWindowConfig = serde_json::from_str(json).unwrap();
        match cfg.height {
            Some(PinHeight::Number(n)) => assert_eq!(n, 100),
            other => panic!("expected Number(100), got {:?}", other),
        }
    }

    #[test]
    fn test_pin_height_auto_string_deserialize() {
        let json = r#"{"height": "auto"}"#;
        let cfg: PinWindowConfig = serde_json::from_str(json).unwrap();
        match cfg.height {
            Some(PinHeight::Auto(s)) => assert_eq!(s, "auto"),
            other => panic!("expected Auto(\"auto\"), got {:?}", other),
        }
    }

    #[test]
    fn test_pin_height_string_number_falls_back_to_auto() {
        // 字符串数字 "100" 被 untagged 当作 Auto(String)，validate 会拒绝。
        let json = r#"{"height": "100"}"#;
        let cfg: PinWindowConfig = serde_json::from_str(json).unwrap();
        match cfg.height {
            Some(PinHeight::Auto(s)) => assert_eq!(s, "100"),
            other => panic!("expected Auto(\"100\"), got {:?}", other),
        }
    }

    // ---------- 边界值与混合 block 测试 ----------

    #[test]
    fn test_mixed_blocks() {
        // 多 block 混排（Phase 2 核心能力）
        let mut doc = valid_doc();
        let abs_path = if cfg!(windows) {
            "C:\\abs\\img.png"
        } else {
            "/abs/img.png"
        };
        doc.blocks = vec![
            PinBlock::Markdown(MarkdownBlock {
                content: "title".to_string(),
            }),
            PinBlock::Image(ImageBlock {
                path: abs_path.to_string(),
                caption: Some("fig".to_string()),
            }),
            PinBlock::Status(StatusBlock {
                level: Some("info".to_string()),
                text: "ok".to_string(),
            }),
        ];
        assert!(validate(&doc).is_ok());
    }

    #[test]
    fn test_image_path_whitespace_only() {
        let mut doc = valid_doc();
        let abs_path = if cfg!(windows) {
            "C:\\abs\\   "
        } else {
            "/abs/   "
        };
        doc.blocks = vec![PinBlock::Image(ImageBlock {
            path: abs_path.to_string(),
            caption: None,
        })];
        assert!(validate(&doc).is_err());
    }

    #[test]
    fn test_image_path_no_extension() {
        let mut doc = valid_doc();
        let abs_path = if cfg!(windows) {
            "C:\\abs\\img"
        } else {
            "/abs/img"
        };
        doc.blocks = vec![PinBlock::Image(ImageBlock {
            path: abs_path.to_string(),
            caption: None,
        })];
        let err = validate(&doc).unwrap_err();
        assert_eq!(err.code, PinErrorCode::ImageUnsupported);
    }

    #[test]
    fn test_markdown_content_whitespace_only() {
        let mut doc = valid_doc();
        doc.blocks = vec![PinBlock::Markdown(MarkdownBlock {
            content: "   \n  ".to_string(),
        })];
        assert!(validate(&doc).is_err());
    }

    #[test]
    fn test_window_width_too_large() {
        let mut doc = valid_doc();
        doc.window = Some(PinWindowConfig {
            width: Some(MAX_WINDOW_DIMENSION + 1),
            height: None,
            x: None,
            y: None,
            always_on_top: None,
        });
        assert!(validate(&doc).is_err());
    }

    #[test]
    fn test_title_exactly_at_limit() {
        let mut doc = valid_doc();
        doc.title = "a".repeat(MAX_TITLE_CHARS);
        assert!(validate(&doc).is_ok());
    }

    #[test]
    fn test_blocks_exactly_at_limit() {
        let mut doc = valid_doc();
        doc.blocks = (0..MAX_BLOCKS)
            .map(|_| {
                PinBlock::Markdown(MarkdownBlock {
                    content: "x".to_string(),
                })
            })
            .collect();
        assert!(validate(&doc).is_ok());
    }

    #[test]
    fn test_caption_too_long() {
        let mut doc = valid_doc();
        let abs_path = if cfg!(windows) {
            "C:\\abs\\img.png"
        } else {
            "/abs/img.png"
        };
        doc.blocks = vec![PinBlock::Image(ImageBlock {
            path: abs_path.to_string(),
            caption: Some("a".repeat(MAX_CAPTION_CHARS + 1)),
        })];
        assert!(validate(&doc).is_err());
    }

    #[test]
    fn test_status_text_too_long() {
        let mut doc = valid_doc();
        doc.blocks = vec![PinBlock::Status(StatusBlock {
            level: None,
            text: "a".repeat(MAX_STATUS_TEXT_CHARS + 1),
        })];
        assert!(validate(&doc).is_err());
    }

    #[test]
    fn test_source_field_too_long() {
        let mut doc = valid_doc();
        doc.source = Some(PinSource {
            agent: Some("a".repeat(MAX_SOURCE_FIELD_CHARS + 1)),
            workspace: None,
            task: None,
            conversation_id: None,
        });
        assert!(validate(&doc).is_err());
    }

    // ---------- M7/M8 新增：path 长度、控制字符、window x/y 范围 ----------

    #[test]
    fn test_image_path_too_long() {
        let mut doc = valid_doc();
        let abs_path = if cfg!(windows) {
            format!("C:\\abs\\{}.png", "a".repeat(MAX_IMAGE_PATH_BYTES))
        } else {
            format!("/abs/{}.png", "a".repeat(MAX_IMAGE_PATH_BYTES))
        };
        doc.blocks = vec![PinBlock::Image(ImageBlock {
            path: abs_path,
            caption: None,
        })];
        assert!(validate(&doc).is_err());
    }

    #[test]
    fn test_title_control_chars_rejected() {
        let mut doc = valid_doc();
        doc.title = "hello\0world".to_string();
        assert!(validate(&doc).is_err());
    }

    #[test]
    fn test_markdown_content_allows_newline() {
        let mut doc = valid_doc();
        doc.blocks = vec![PinBlock::Markdown(MarkdownBlock {
            content: "line1\nline2\ttabbed".to_string(),
        })];
        assert!(validate(&doc).is_ok());
    }

    #[test]
    fn test_markdown_content_null_byte_rejected() {
        let mut doc = valid_doc();
        doc.blocks = vec![PinBlock::Markdown(MarkdownBlock {
            content: "evil\0content".to_string(),
        })];
        assert!(validate(&doc).is_err());
    }

    #[test]
    fn test_image_path_null_byte_rejected() {
        let mut doc = valid_doc();
        doc.blocks = vec![PinBlock::Image(ImageBlock {
            path: "C:\\safe\\img.png\0.evil".to_string(),
            caption: None,
        })];
        assert!(validate(&doc).is_err());
    }

    #[test]
    fn test_window_x_out_of_range() {
        let mut doc = valid_doc();
        doc.window = Some(PinWindowConfig {
            width: None,
            height: None,
            x: Some(MAX_WINDOW_POS + 1),
            y: None,
            always_on_top: None,
        });
        assert!(validate(&doc).is_err());
    }

    #[test]
    fn test_window_y_negative_extreme() {
        let mut doc = valid_doc();
        doc.window = Some(PinWindowConfig {
            width: None,
            height: None,
            x: None,
            y: Some(MIN_WINDOW_POS - 1),
            always_on_top: None,
        });
        assert!(validate(&doc).is_err());
    }

    #[test]
    fn test_window_x_at_boundary() {
        let mut doc = valid_doc();
        doc.window = Some(PinWindowConfig {
            width: None,
            height: None,
            x: Some(MAX_WINDOW_POS),
            y: Some(MIN_WINDOW_POS),
            always_on_top: None,
        });
        assert!(validate(&doc).is_ok());
    }

    #[test]
    fn test_pin_height_string_number_error_hint() {
        let mut doc = valid_doc();
        doc.window = Some(PinWindowConfig {
            width: None,
            height: Some(PinHeight::Auto("100".to_string())),
            x: None,
            y: None,
            always_on_top: None,
        });
        let err = validate(&doc).unwrap_err();
        // 错误信息应包含提示用数字而非字符串
        assert!(err.message.contains("use JSON number"));
    }
}
