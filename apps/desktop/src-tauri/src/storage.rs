// Pin 持久化层
//
// 数据目录：~/.agent-pin/（Windows: %USERPROFILE%\.agent-pin\）
//   pins/{pinId}.json - 每个 Pin 的完整 PinDocument
//   state.json        - 所有 Pin 的元数据列表（轻量索引）
//
// 设计原则：
// - state.json 只存元数据（pinId/title/createdAt/updatedAt/state/source），
//   完整 doc 存 pins/{pinId}.json。这样管理界面列表只需读 state.json，
//   无需扫描所有 pins/{pinId}.json。
// - 所有写操作原子化：先写 .tmp，再 fs::rename（Windows 同目录 rename 是原子替换）。
// - 启动时加载 state.json 到内存 registry；pins/{pinId}.json 在 show 时按需读取。
// - 坏 state.json 不阻塞应用启动，降级为空状态并 eprintln。
//
// Phase 2-B 不实现 inbox/ 和 failed/ 目录（architecture.md §5 留给未来场景）。
//
// 契约来源：docs/architecture.md §5、docs/phase-plan.md Phase 2-B

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::pin::PinDocument;

// ---------- Pin 状态 ----------

/// Pin 状态（持久化到 state.json）。
/// 生命周期：created → visible ↔ hidden；created → failed。
/// MVP 不做 deleted 状态（删除即从 state.json 移除记录 + 删 pins/{pinId}.json）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PinState {
    /// 窗口当前可见
    Visible,
    /// 窗口已关闭，记录保留（可恢复）
    Hidden,
    /// 创建窗口失败（保留记录供管理界面查看/删除）
    Failed,
}

// ---------- state.json 结构 ----------

/// state.json 中的单条 Pin 元数据。
/// 用于管理界面列表 + 托盘快恢列表，无需读取 pins/{pinId}.json。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinMeta {
    pub pin_id: String,
    pub title: String,
    /// ISO 8601 时间字符串（与 PinDocument.createdAt 同步）
    pub created_at: String,
    /// 状态变更时间（ISO 8601），用于排序"最近"
    pub updated_at: String,
    pub state: PinState,
    /// source 用于管理界面按 agent/workspace 过滤。
    /// 与 PinDocument.source 同步：插入时一次性写入，后续不修改。
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub source: Option<crate::pin::PinSource>,
}

/// state.json 整体结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateFile {
    pub version: u32,
    /// 按 createdAt 升序存储（最早的在前），查询最近时反转。
    /// 插入新 Pin 时直接 push 到末尾，O(1)。
    pub pins: Vec<PinMeta>,
}

impl Default for StateFile {
    fn default() -> Self {
        Self {
            version: 1,
            pins: Vec::new(),
        }
    }
}

// ---------- 路径 ----------

/// 跨平台获取用户 home 目录。
/// Windows: %USERPROFILE%；Unix: $HOME。
/// 不引入 dirs crate，手写够用且避免新依赖。
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// 数据目录：~/.agent-pin/
/// home_dir 几乎不可能为 None（Windows 总有 USERPROFILE，Unix 总有 HOME）。
/// 若真为 None，eprintln 警告并 fallback 到当前目录，避免静默写入意外位置。
/// 不返回 Result 是为了避免所有调用方（init/load/save/delete）都要处理 Err，
/// 实际 None 场景下后续 I/O 会自然失败并被调用方的错误处理捕获。
pub fn data_dir() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| {
            eprintln!(
                "[agent-pin] HOME/USERPROFILE not set, falling back to current directory"
            );
            PathBuf::from(".")
        })
        .join(".agent-pin")
}

pub fn pins_dir() -> PathBuf {
    data_dir().join("pins")
}

pub fn state_file_path() -> PathBuf {
    data_dir().join("state.json")
}

pub fn pin_file_path(pin_id: &str) -> PathBuf {
    pins_dir().join(format!("{}.json", pin_id))
}

/// 校验 pin_id 格式，防路径穿越（M1）。
/// pin_id 由 generate_pin_id 生成，格式为 pin_<timestamp>_<6位随机>。
/// 这里做防御性校验：拒绝空字符串、包含路径分隔符或 `..` 的输入，
/// 避免 pin_id = "../state" 等导致删除/读取非目标文件。
/// 同时拒绝 NUL 字节（%00 解码后），避免文件名截断风险。
pub fn validate_pin_id(pin_id: &str) -> Result<(), String> {
    if pin_id.is_empty() {
        return Err("pin_id must be non-empty".to_string());
    }
    if pin_id.contains('/') || pin_id.contains('\\') || pin_id.contains("..") || pin_id.contains('\0') {
        return Err(format!("invalid pin_id (path traversal detected): {}", pin_id));
    }
    Ok(())
}

// ---------- 初始化 ----------

/// 初始化数据目录（启动时调用）。
/// 创建 ~/.agent-pin/pins/，state.json 在首次 save 时创建。
pub fn init() -> std::io::Result<()> {
    fs::create_dir_all(pins_dir())?;
    Ok(())
}

// ---------- state.json I/O ----------

/// 加载 state.json。
/// - 文件不存在：返回空 StateFile（首次启动）
/// - 解析失败：eprintln + 返回空 StateFile（坏 state.json 不阻塞启动）
/// - 读取失败（非 NotFound）：eprintln + 返回空 StateFile
pub fn load_state() -> StateFile {
    let path = state_file_path();
    match fs::read_to_string(&path) {
        Ok(s) => match serde_json::from_str::<StateFile>(&s) {
            Ok(state) => state,
            Err(e) => {
                eprintln!(
                    "[agent-pin] state.json 解析失败，降级为空状态: {} (path={})",
                    e,
                    path.display()
                );
                StateFile::default()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => StateFile::default(),
        Err(e) => {
            eprintln!(
                "[agent-pin] state.json 读取失败，降级为空状态: {} (path={})",
                e,
                path.display()
            );
            StateFile::default()
        }
    }
}

/// 保存 state.json（原子写：先写 .tmp，再 rename）。
/// 失败时 eprintln 并返回 Err，调用方决定是否重试或降级。
pub fn save_state(state: &StateFile) -> std::io::Result<()> {
    let path = state_file_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let s = serde_json::to_string_pretty(state)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    fs::write(&tmp, s)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

// ---------- pins/{pinId}.json I/O ----------

/// 保存单个 Pin 的 PinDocument 到 pins/{pinId}.json（原子写）。
pub fn save_pin_doc(pin_id: &str, doc: &PinDocument) -> std::io::Result<()> {
    let path = pin_file_path(pin_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let s = serde_json::to_string_pretty(doc)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    fs::write(&tmp, s)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

/// 读取单个 Pin 的 PinDocument。
/// 文件不存在或解析失败返回 None，解析失败额外 eprintln。
pub fn load_pin_doc(pin_id: &str) -> Option<PinDocument> {
    let path = pin_file_path(pin_id);
    let s = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            eprintln!(
                "[agent-pin] pins/{}.json 读取失败: {} (path={})",
                pin_id,
                e,
                path.display()
            );
            return None;
        }
    };
    match serde_json::from_str::<PinDocument>(&s) {
        Ok(doc) => Some(doc),
        Err(e) => {
            eprintln!(
                "[agent-pin] pins/{}.json 解析失败: {} (path={})",
                pin_id,
                e,
                path.display()
            );
            None
        }
    }
}

/// 删除单个 Pin 文件。文件不存在不算错误（幂等）。
pub fn delete_pin_doc(pin_id: &str) -> std::io::Result<()> {
    let path = pin_file_path(pin_id);
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}
