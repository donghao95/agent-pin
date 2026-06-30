// Pin 数据模型与校验逻辑
//
// 这是 API / CLI / 窗口渲染之间的核心契约。
// Phase 2-A 起接受 markdown / image / status block，支持多 block 混排。
//
// 契约来源：docs/mvp-spec.md §8、docs/api.md §9

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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[allow(dead_code)] // WindowCreateFailed/InternalError 预留给 Phase 2-B/2-C
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
    /// Phase 2-B：show/hide/delete 路由中 pinId 不存在。
    PinNotFound,
    WindowCreateFailed,
    InternalError,
}

/// 支持的图片扩展名白名单（docs/mvp-spec.md §9）。
/// 同时用于 HTTP 校验和前端防御，避免 asset protocol 加载非图片文件。
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif"];

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
/// Phase 2-A 起接受 markdown / image / status block，支持多 block 混排。
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
    // 校验 window.height：如果是字符串变体，值必须恰好是 "auto"。
    // PinHeight 用 untagged 反序列化，任何字符串都会被当作 Auto，需要在这里拦截非法值，
    // 避免 "tall"/"100px" 等错误输入被静默当作 auto 处理。
    if let Some(win) = &doc.window {
        if let Some(PinHeight::Auto(s)) = &win.height {
            if s != "auto" {
                return Err(PinError::new(
                    PinErrorCode::InvalidPinDocument,
                    format!("window.height string must be \"auto\", got {:?}", s),
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
    for (i, block) in doc.blocks.iter().enumerate() {
        match block {
            PinBlock::Markdown(m) => {
                if m.content.trim().is_empty() {
                    return Err(PinError::new(
                        PinErrorCode::InvalidPinDocument,
                        format!("blocks[{}].content must be non-empty", i),
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
                // path 必须是绝对路径（docs/mvp-spec.md §9）。
                // 不校验文件存在性：desktop 不知道 Agent cwd，前端 <img> onerror 处理。
                if !Path::new(&img.path).is_absolute() {
                    return Err(PinError::new(
                        PinErrorCode::InvalidPinDocument,
                        format!("blocks[{}].path must be absolute (relative path resolution is CLI's job in Phase 2-C)", i),
                    ));
                }
                // 扩展名白名单校验（docs/mvp-spec.md §9：PNG/JPG/JPEG/WebP/GIF）。
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
