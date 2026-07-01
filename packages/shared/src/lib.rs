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
/// 窗口宽度/高度数值下限（防 0 或极小值导致不可见窗口）。
pub const MIN_WINDOW_DIMENSION: u32 = 1;
/// 窗口宽度/高度数值上限（防超出屏幕导致创建失败）。
pub const MAX_WINDOW_DIMENSION: u32 = 100_000;

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
        Self { code, message: message.into() }
    }
}

// ---------- 校验 ----------

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
    // 校验 window 数值范围：width/height(Number) 必须在 [MIN, MAX] 内，
    // 防 0 或极小值导致不可见窗口，防超大值导致创建失败（M11）。
    if let Some(win) = &doc.window {
        if let Some(w) = win.width {
            if w < MIN_WINDOW_DIMENSION || w > MAX_WINDOW_DIMENSION {
                return Err(PinError::new(
                    PinErrorCode::InvalidPinDocument,
                    format!(
                        "window.width must be in [{}..{}], got {}",
                        MIN_WINDOW_DIMENSION, MAX_WINDOW_DIMENSION, w
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
                return Err(PinError::new(
                    PinErrorCode::InvalidPinDocument,
                    format!("window.height string must be \"auto\", got {:?}", s),
                ));
            }
        }
        if let Some(PinHeight::Number(n)) = &win.height {
            if *n < MIN_WINDOW_DIMENSION || *n > MAX_WINDOW_DIMENSION {
                return Err(PinError::new(
                    PinErrorCode::InvalidPinDocument,
                    format!(
                        "window.height must be in [{}..{}], got {}",
                        MIN_WINDOW_DIMENSION, MAX_WINDOW_DIMENSION, n
                    ),
                ));
            }
        }
        // x/y 数值范围不校验：负数合法（可离屏），超大值由 OS 处理。
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
                if let Some(level) = &s.level {
                    if !matches!(level.as_str(), "info" | "success" | "warning" | "error") {
                        return Err(PinError::new(
                            PinErrorCode::InvalidPinDocument,
                            format!("blocks[{}].level must be one of info/success/warning/error", i),
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
            .map(|_| PinBlock::Markdown(MarkdownBlock { content: "x".to_string() }))
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
        assert!(validate(&doc).is_ok(), "uppercase extension should be valid");
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
}
