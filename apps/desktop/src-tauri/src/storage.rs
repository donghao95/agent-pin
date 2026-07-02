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
// Phase 2-B 不实现 inbox/ 和 failed/ 目录（docs/02_architecture.md §5 留给未来场景）。
//
// 契约来源：docs/02_architecture.md §5、docs/06_phase_plan.md Phase 2-B

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

/// 用户手动调整后的窗口尺寸（持久化到 state.json）。
/// show 时优先用此尺寸恢复窗口，而非 doc.window 或默认值。
/// 仅记录用户主动 resize 后的尺寸，fit_pin_window_height 的自动调整不记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowSize {
    pub width: f64,
    pub height: f64,
}

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
    /// 用户手动调整后的窗口尺寸（宽高，逻辑像素）。
    /// show 时优先用此尺寸恢复窗口。
    /// None 表示用户未手动调整过，用 doc.window 或默认值。
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub window_size: Option<WindowSize>,
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

/// 返回用户 home 目录（带 fallback）。
/// home_dir 几乎不可能为 None（Windows 总有 USERPROFILE，Unix 总有 HOME）。
/// 若真为 None，eprintln 警告并 fallback 到当前目录，避免静默写入意外位置。
///
/// production I/O 函数（init/load_state/save_state 等）转发到 _to/_for 版本时
/// 传此函数结果（home 目录），与 data_dir_for(root) 语义一致（root 当 home）。
/// 测试传 tempdir（充当 home），实现测试隔离。
///
/// 注意：不要传 data_dir() 给 _to/_for 版本——data_dir_for(root) 的语义是
/// "root 当 home，返回 root/.agent-pin"，传 data_dir()（已是 ~/.agent-pin）
/// 会导致路径多套一层 .agent-pin（B1 bug）。
pub(crate) fn home_or_fallback() -> PathBuf {
    home_dir().unwrap_or_else(|| {
        eprintln!("[agent-pin] HOME/USERPROFILE not set, falling back to current directory");
        PathBuf::from(".")
    })
}

/// 数据目录：~/.agent-pin/
/// 不返回 Result 是为了避免所有调用方（init/load/save/delete）都要处理 Err，
/// 实际 None 场景下后续 I/O 会自然失败并被调用方的错误处理捕获。
pub fn data_dir() -> PathBuf {
    home_or_fallback().join(".agent-pin")
}

/// 在指定 root 下计算数据目录路径（测试用）。
/// production 代码调 data_dir()（读 HOME 环境变量），测试调 data_dir_for(tempdir)
/// 避免污染真实文件系统或并发测试串扰（env var 是进程级全局）。
pub(crate) fn data_dir_for(root: &std::path::Path) -> PathBuf {
    root.join(".agent-pin")
}

pub fn pins_dir() -> PathBuf {
    data_dir().join("pins")
}

/// 在指定 root 下计算 pins 目录路径（测试用）。
pub(crate) fn pins_dir_for(root: &std::path::Path) -> PathBuf {
    data_dir_for(root).join("pins")
}

#[allow(dead_code)]
pub fn state_file_path() -> PathBuf {
    data_dir().join("state.json")
}

/// 在指定 root 下计算 state.json 路径（测试用）。
pub(crate) fn state_file_path_for(root: &std::path::Path) -> PathBuf {
    data_dir_for(root).join("state.json")
}

#[allow(dead_code)]
pub fn pin_file_path(pin_id: &str) -> PathBuf {
    pins_dir().join(format!("{}.json", pin_id))
}

/// 在指定 root 下计算 pin doc 路径（测试用）。
pub(crate) fn pin_file_path_for(root: &std::path::Path, pin_id: &str) -> PathBuf {
    pins_dir_for(root).join(format!("{}.json", pin_id))
}

/// 校验 pin_id 格式，防路径穿越和保留 label 滥用（m2）。
/// pin_id 由 generate_pin_id 生成，格式为 pin_<timestamp>_<6位随机>。
/// 这里做防御性校验：
/// - 拒绝空字符串、包含路径分隔符或 `..` 的输入（防路径穿越）
/// - 拒绝 NUL 字节（%00 解码后），避免文件名截断风险
/// - 拒绝超长 pin_id（防 HashMap 内存膨胀）
/// - 拒绝保留 label "manager"（防止通过 Tauri invoke 销毁管理窗口）
pub fn validate_pin_id(pin_id: &str) -> Result<(), String> {
    if pin_id.is_empty() {
        return Err("pin_id must be non-empty".to_string());
    }
    if pin_id.len() > 128 {
        return Err(format!(
            "pin_id too long (max 128 chars): {} chars",
            pin_id.len()
        ));
    }
    if pin_id.contains('/')
        || pin_id.contains('\\')
        || pin_id.contains("..")
        || pin_id.contains('\0')
    {
        return Err(format!(
            "invalid pin_id (path traversal detected): {}",
            pin_id
        ));
    }
    if pin_id == "manager" {
        return Err("pin_id 'manager' is reserved and cannot be used".to_string());
    }
    Ok(())
}

// ---------- 初始化 ----------

/// 初始化数据目录（启动时调用）。
/// 创建 ~/.agent-pin/pins/，state.json 在首次 save 时创建。
pub fn init() -> std::io::Result<()> {
    init_to(&home_or_fallback())
}

/// 在指定 root 下初始化数据目录（测试用）。
pub(crate) fn init_to(root: &std::path::Path) -> std::io::Result<()> {
    fs::create_dir_all(pins_dir_for(root))?;
    Ok(())
}

// ---------- state.json I/O ----------

/// 加载 state.json。
/// - 文件不存在：返回空 StateFile（首次启动）
/// - 解析失败：eprintln + 返回空 StateFile（坏 state.json 不阻塞启动）
/// - 读取失败（非 NotFound）：eprintln + 返回空 StateFile
pub fn load_state() -> StateFile {
    load_state_from(&home_or_fallback())
}

/// 在指定 root 下加载 state.json（测试用）。
pub(crate) fn load_state_from(root: &std::path::Path) -> StateFile {
    let path = state_file_path_for(root);
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

/// 保存 state.json（原子写：先写 .tmp，fsync，再 rename）。
/// 失败时 eprintln 并返回 Err，调用方决定是否重试或降级。
pub fn save_state(state: &StateFile) -> std::io::Result<()> {
    save_state_to(&home_or_fallback(), state)
}

/// 在指定 root 下保存 state.json（测试用）。
pub(crate) fn save_state_to(root: &std::path::Path, state: &StateFile) -> std::io::Result<()> {
    let path = state_file_path_for(root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let s = serde_json::to_string_pretty(state).map_err(std::io::Error::other)?;
    // 原子写：write + fsync + rename，确保数据落盘后再替换
    write_and_sync(&tmp, &s)?;
    if let Err(e) = fs::rename(&tmp, &path) {
        // rename 失败时清理残留 tmp 文件，避免堆积
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

// ---------- pins/{pinId}.json I/O ----------

/// 保存单个 Pin 的 PinDocument 到 pins/{pinId}.json（原子写 + fsync）。
pub fn save_pin_doc(pin_id: &str, doc: &PinDocument) -> std::io::Result<()> {
    save_pin_doc_to(&home_or_fallback(), pin_id, doc)
}

/// 在指定 root 下保存 PinDocument（测试用）。
pub(crate) fn save_pin_doc_to(
    root: &std::path::Path,
    pin_id: &str,
    doc: &PinDocument,
) -> std::io::Result<()> {
    let path = pin_file_path_for(root, pin_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let s = serde_json::to_string_pretty(doc).map_err(std::io::Error::other)?;
    write_and_sync(&tmp, &s)?;
    if let Err(e) = fs::rename(&tmp, &path) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// 读取单个 Pin 的 PinDocument。
/// 文件不存在或解析失败返回 None，解析失败额外 eprintln。
pub fn load_pin_doc(pin_id: &str) -> Option<PinDocument> {
    load_pin_doc_from(&home_or_fallback(), pin_id)
}

/// 在指定 root 下读取 PinDocument（测试用）。
pub(crate) fn load_pin_doc_from(root: &std::path::Path, pin_id: &str) -> Option<PinDocument> {
    let path = pin_file_path_for(root, pin_id);
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
    delete_pin_doc_from(&home_or_fallback(), pin_id)
}

/// 在指定 root 下删除 Pin 文件（测试用）。
pub(crate) fn delete_pin_doc_from(root: &std::path::Path, pin_id: &str) -> std::io::Result<()> {
    let path = pin_file_path_for(root, pin_id);
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

/// 写入文件并 fsync，确保数据落盘后再原子 rename。
/// fsync 保证文件内容（不只是元数据）写入存储设备，
/// 避免崩溃后 rename 完成但内容为空的部分写场景。
fn write_and_sync(path: &std::path::Path, content: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::File::create(path)?;
    f.write_all(content.as_bytes())?;
    f.sync_all()?;
    Ok(())
}

// ---------- 测试 ----------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_pin_id_valid() {
        // 标准 pin_id 格式
        assert!(validate_pin_id("pin_1234567890_000001").is_ok());
        assert!(validate_pin_id("pin_0_999999").is_ok());
        // 普通的字母数字组合（不要求严格格式，只防路径穿越）
        assert!(validate_pin_id("abc123").is_ok());
    }

    #[test]
    fn test_validate_pin_id_empty() {
        assert!(validate_pin_id("").is_err());
    }

    #[test]
    fn test_validate_pin_id_path_traversal() {
        // 正斜杠
        assert!(validate_pin_id("pin_123/sub").is_err());
        // 反斜杠
        assert!(validate_pin_id("pin_123\\sub").is_err());
        // .. 穿越
        assert!(validate_pin_id("..").is_err());
        assert!(validate_pin_id("pin_../../state").is_err());
        // NUL 字节（防 %00 截断）
        assert!(validate_pin_id("pin_123\0evil").is_err());
    }

    #[test]
    fn test_validate_pin_id_too_long() {
        let long_id = "a".repeat(129);
        assert!(validate_pin_id(&long_id).is_err());
    }

    #[test]
    fn test_validate_pin_id_manager_reserved() {
        assert!(validate_pin_id("manager").is_err());
    }

    #[test]
    fn test_validate_pin_id_at_length_limit() {
        let id = "a".repeat(128);
        assert!(validate_pin_id(&id).is_ok());
    }

    #[test]
    fn test_pin_file_path_construction() {
        let path = pin_file_path("pin_123_000001");
        assert!(path.to_string_lossy().ends_with("pin_123_000001.json"));
        assert!(path.to_string_lossy().contains("pins"));
    }

    #[test]
    fn test_state_file_default() {
        let state = StateFile::default();
        assert_eq!(state.version, 1);
        assert!(state.pins.is_empty());
    }

    #[test]
    fn test_production_forwarding_path_consistency() {
        // B1 防护：production I/O 函数转发到 _to/_for 版本时传 home_or_fallback()，
        // data_dir_for(home) = home/.agent-pin 必须等于 data_dir() = home_or_fallback().join(".agent-pin")。
        // 若有人误把转发参数改回 data_dir()，data_dir_for(data_dir()) = data_dir()/.agent-pin，
        // 此测试会失败，防止 B1 路径叠加 bug 再次发生。
        let home = home_or_fallback();
        assert_eq!(
            data_dir_for(&home),
            data_dir(),
            "data_dir_for(home) must equal data_dir() — production forwarding broken"
        );
    }

    // ---------- P1: 持久化 I/O 测试（用 data_dir_for 注入 tempdir） ----------

    /// 辅助：构造临时 root 目录，测试结束后自动清理（含 panic 时）。
    /// 用 tempfile::TempDir 避免 Windows SystemTime 精度低导致的并发路径冲突。
    fn with_temp_root(f: impl FnOnce(&std::path::Path)) {
        let tmp = tempfile::TempDir::new().expect("create tempdir");
        f(tmp.path());
        // tmp drop 时自动清理
    }

    #[test]
    fn test_init_creates_pins_dir() {
        with_temp_root(|root| {
            init_to(root).expect("init should succeed");
            assert!(pins_dir_for(root).exists(), "pins dir should be created");
        });
    }

    #[test]
    fn test_save_and_load_state_roundtrip() {
        with_temp_root(|root| {
            let state = StateFile {
                version: 1,
                pins: vec![PinMeta {
                    pin_id: "pin_123_000001".to_string(),
                    title: "Test".to_string(),
                    created_at: "2026-01-01T00:00:00Z".to_string(),
                    updated_at: "2026-01-02T00:00:00Z".to_string(),
                    state: PinState::Hidden,
                    source: None,
                    window_size: None,
                }],
            };
            save_state_to(root, &state).expect("save should succeed");

            let loaded = load_state_from(root);
            assert_eq!(loaded.version, 1);
            assert_eq!(loaded.pins.len(), 1);
            assert_eq!(loaded.pins[0].pin_id, "pin_123_000001");
            assert_eq!(loaded.pins[0].title, "Test");
            assert_eq!(loaded.pins[0].state, PinState::Hidden);
        });
    }

    #[test]
    fn test_load_state_missing_file_returns_default() {
        with_temp_root(|root| {
            let loaded = load_state_from(root);
            assert_eq!(loaded.version, 1);
            assert!(loaded.pins.is_empty());
        });
    }

    #[test]
    fn test_load_state_corrupt_json_returns_default() {
        with_temp_root(|root| {
            std::fs::create_dir_all(data_dir_for(root)).unwrap();
            std::fs::write(state_file_path_for(root), "{ not valid json").unwrap();

            let loaded = load_state_from(root);
            assert_eq!(loaded.version, 1);
            assert!(loaded.pins.is_empty());
        });
    }

    #[test]
    fn test_save_and_load_pin_doc_roundtrip() {
        with_temp_root(|root| {
            let doc = PinDocument {
                version: 1,
                title: "Test Pin".to_string(),
                blocks: vec![crate::pin::PinBlock::Markdown(crate::pin::MarkdownBlock {
                    content: "## Hello".to_string(),
                })],
                window: None,
                source: None,
                created_at: Some("2026-01-01T00:00:00Z".to_string()),
            };
            save_pin_doc_to(root, "pin_123_000001", &doc).expect("save should succeed");

            let loaded = load_pin_doc_from(root, "pin_123_000001");
            assert!(loaded.is_some());
            let loaded = loaded.unwrap();
            assert_eq!(loaded.title, "Test Pin");
        });
    }

    #[test]
    fn test_load_pin_doc_missing_returns_none() {
        with_temp_root(|root| {
            assert!(load_pin_doc_from(root, "pin_nonexistent").is_none());
        });
    }

    #[test]
    fn test_load_pin_doc_corrupt_returns_none() {
        with_temp_root(|root| {
            std::fs::create_dir_all(pins_dir_for(root)).unwrap();
            std::fs::write(pin_file_path_for(root, "pin_123_000001"), "{ broken").unwrap();

            assert!(load_pin_doc_from(root, "pin_123_000001").is_none());
        });
    }

    #[test]
    fn test_delete_pin_doc_idempotent() {
        with_temp_root(|root| {
            std::fs::create_dir_all(pins_dir_for(root)).unwrap();
            // 删除不存在的文件应返回 Ok（幂等）
            assert!(delete_pin_doc_from(root, "pin_nonexistent").is_ok());

            // 删除已存在的文件
            let path = pin_file_path_for(root, "pin_123_000001");
            std::fs::write(&path, "{}").unwrap();
            assert!(delete_pin_doc_from(root, "pin_123_000001").is_ok());
            assert!(!path.exists());
        });
    }
}
