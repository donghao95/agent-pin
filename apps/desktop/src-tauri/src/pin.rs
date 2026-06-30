// Pin 数据模型与校验逻辑
//
// 这是 API / CLI / 窗口渲染之间的核心契约。
// Phase 1 只接受 markdown block，但类型定义完整以便 Phase 2 扩展时不必改契约。
//
// 契约来源：docs/mvp-spec.md §8、docs/api.md §9

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
#[allow(dead_code)] // Phase 2 会用到 image / internal 错误码
pub enum PinErrorCode {
    InvalidJson,
    InvalidPinDocument,
    UnsupportedBlockType,
    ImageNotFound,
    ImageUnsupported,
    WindowCreateFailed,
    InternalError,
}

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
/// Phase 1 只接受 markdown block；image / status 返回 UnsupportedBlockType。
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
            PinBlock::Image(_) => {
                return Err(PinError::new(
                    PinErrorCode::UnsupportedBlockType,
                    "image block is not supported in Phase 1",
                ));
            }
            PinBlock::Status(_) => {
                return Err(PinError::new(
                    PinErrorCode::UnsupportedBlockType,
                    "status block is not supported in Phase 1",
                ));
            }
        }
    }
    Ok(())
}
