// Pin 内存注册表 + 持久化集成
//
// 职责：
// - 以 pinId 为 key 保存 PinEntry（doc + meta），供前端 invoke 读取
// - 集成持久化：insert / set_state / remove 时同步写 state.json 和 pins/{pinId}.json
// - 启动时从磁盘加载历史 Pin（load_from_disk）
//
// 设计：
// - 全局单例（once_cell::Lazy），HTTP handler 和 Tauri invoke 共用。
//   选型理由：axum handler 和 Tauri invoke 是两套 state 体系，全局单例最简单且单进程无并发问题。
// - 内存中持有完整 PinEntry（doc + meta），避免每次 get 都读磁盘。
// - state.json 在每次变更后从内存重新生成（Pin 数量少，性能可接受）。
//
// Phase 2-B 改动（相对 Phase 1）：
// - 从纯内存改为内存 + 持久化
// - 关闭窗口 = set_state(hidden)，不删除记录
// - 删除 Pin = remove（删记录 + 删 pins/{pinId}.json + 更新 state.json）
// - 启动时 load_from_disk 恢复历史
//
// 契约来源：docs/architecture.md §3/§5、docs/phase-plan.md Phase 2-B

use std::collections::HashMap;
use std::sync::Mutex;

use once_cell::sync::Lazy;

use crate::pin::PinDocument;
use crate::storage::{self, PinMeta, PinState, StateFile};

// ---------- PinEntry ----------

/// 内存中的 Pin 完整记录。
/// doc 用于渲染，meta 用于列表/状态管理。
#[derive(Clone)]
pub struct PinEntry {
    pub doc: PinDocument,
    pub meta: PinMeta,
}

// ---------- PinRegistry ----------

pub struct PinRegistry {
    inner: Mutex<HashMap<String, PinEntry>>,
}

impl PinRegistry {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// 插入新 Pin（state=visible）。
    /// 顺序：
    ///   1. 写 pins/{pinId}.json（失败则不插入内存，返回 Err）
    ///   2. 加锁，插入内存 entry
    /// 3. 写 state.json（失败则回滚：移除内存 entry + 删 pins/{pinId}.json + 返回 Err）
    ///
    /// 回滚理由：state.json 是重启后 load_from_disk 的唯一来源，若写失败而不回滚，
    /// 重启后该 Pin 不在 state.json 中，pins/{pinId}.json 成为永久孤儿文件，且用户无法通过
    /// 管理界面看到或删除它（管理界面读内存 list()，而内存会在重启后从 state.json 重建）。
    pub fn insert(&self, pin_id: String, doc: PinDocument) -> Result<PinMeta, String> {
        // 1. 先写 doc（失败则不插入内存，避免内存与磁盘不一致）
        if let Err(e) = storage::save_pin_doc(&pin_id, &doc) {
            return Err(format!("failed to save pin doc: {}", e));
        }

        let meta = build_meta(&pin_id, &doc, PinState::Visible);

        // 2. 加锁，插入内存 + 写 state
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.insert(
            pin_id.clone(),
            PinEntry {
                doc,
                meta: meta.clone(),
            },
        );

        // 3. 写 state（失败则回滚：移除内存 entry，释放锁后删 doc 文件）
        if let Err(e) = persist_state_locked(&inner) {
            inner.remove(&pin_id);
            drop(inner); // 释放锁后再做文件 I/O，避免持锁阻塞其他操作
            if let Err(del_err) = storage::delete_pin_doc(&pin_id) {
                eprintln!(
                    "[agent-pin] rollback delete_pin_doc failed for {}: {}",
                    pin_id, del_err
                );
            }
            return Err(format!("failed to save state.json: {}", e));
        }

        Ok(meta)
    }

    /// 读取 PinDocument（兼容 invoke get_pin_document）。
    /// M4：与 insert/set_state/remove 等方法一致，使用 unwrap_or_else 恢复中毒锁，
    /// 避免 mutex 中毒时 panic 传播到 HTTP handler / Tauri invoke / 窗口事件处理。
    pub fn get(&self, pin_id: &str) -> Option<PinDocument> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(pin_id)
            .map(|e| e.doc.clone())
    }

    /// 读取 PinMeta。
    /// M4：同 get，统一中毒锁恢复策略。
    pub fn get_meta(&self, pin_id: &str) -> Option<PinMeta> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(pin_id)
            .map(|e| e.meta.clone())
    }

    /// 更新 Pin 状态（visible ↔ hidden ↔ failed）。
    /// 同时更新 updated_at 和 state.json。
    /// Pin 不存在时返回 Err。持久化失败时返回 Err 并回滚内存状态（原子性：内存与磁盘一致）。
    /// 回滚理由：若持久化失败但保留内存变更，调用方收到 Err 后无法判断内存状态，
    /// 且重启后磁盘恢复旧状态会造成内存/磁盘长期不一致。回滚保证 set_state 全成功或全失败。
    pub fn set_state(&self, pin_id: &str, state: PinState) -> Result<(), String> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let entry = inner
            .get_mut(pin_id)
            .ok_or_else(|| format!("pin not found: {}", pin_id))?;
        let old_state = entry.meta.state;
        let old_updated_at = entry.meta.updated_at.clone();
        entry.meta.state = state;
        entry.meta.updated_at = now_iso();

        if let Err(e) = persist_state_locked(&inner) {
            eprintln!(
                "[agent-pin] failed to save state.json after set_state (rolling back memory): {}",
                e
            );
            // 回滚内存状态，保证内存 == 最后一次成功落盘的状态
            if let Some(entry) = inner.get_mut(pin_id) {
                entry.meta.state = old_state;
                entry.meta.updated_at = old_updated_at;
            }
            return Err(format!("failed to save state.json: {}", e));
        }
        Ok(())
    }

    /// 删除 Pin（不可恢复）。
    /// 顺序：
    ///   1. 校验 pin_id 格式（防路径穿越）
    ///   2. 加锁，从内存取出 entry（不存在直接返回 Ok，幂等）
    ///   3. 同一锁作用域内写 state.json（已不含此 Pin）。失败则把 entry 放回内存，返回 Err
    /// 4. 释放锁后删 pins/{pinId}.json（best-effort，失败仅 eprintln，不影响结果）
    ///
    /// 设计理由：
    /// - 先持久化 state.json 成功后再删 doc 文件，避免"doc 已删但 state.json 仍引用"
    ///   的不一致（那种状态重启后会让 Pin 以 failed 复活）。doc 删除失败只会留下孤儿文件，
    ///   load_from_disk 不会拾取它（state.json 不引用），无害。
    /// - 步骤 2 和 3 在同一锁作用域内，避免两次加锁之间存在一致性窗口
    ///   （M-1 修复：否则并发 list()/get_meta() 会观测到"内存已无但 state.json 仍有"的瞬态）。
    pub fn remove(&self, pin_id: &str) -> Result<(), String> {
        // 1. 校验 pin_id 格式（防路径穿越，M1）
        storage::validate_pin_id(pin_id)?;

        // 2-3. 加锁，取出 entry + 写 state.json（同一锁作用域，避免一致性窗口）
        {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            let entry = match inner.remove(pin_id) {
                Some(e) => e,
                None => return Ok(()), // 幂等：不存在直接返回
            };

            if let Err(e) = persist_state_locked(&inner) {
                // 回滚：把 entry 放回内存，保持内存与磁盘（旧 state.json）一致
                inner.insert(pin_id.to_string(), entry);
                eprintln!(
                    "[agent-pin] failed to save state.json after remove (rolling back memory): {} (pin_id={})",
                    e, pin_id
                );
                return Err(format!("failed to save state.json: {}", e));
            }
        }

        // 4. 释放锁后删 doc 文件（best-effort，失败仅日志，不影响删除结果）
        if let Err(e) = storage::delete_pin_doc(pin_id) {
            eprintln!(
                "[agent-pin] failed to delete pin doc after state.json removed (orphan file left): {} (pin_id={})",
                e, pin_id
            );
        }
        Ok(())
    }

    /// 列出所有 Pin 的 meta（按 createdAt 降序，最新的在前）。
    /// 用于管理界面完整历史列表。
    pub fn list(&self) -> Vec<PinMeta> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut pins: Vec<PinMeta> = inner.values().map(|e| e.meta.clone()).collect();
        pins.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        pins
    }

    /// 列出最近 n 个 hidden Pin（按 updatedAt 降序，最近变更的在前）。
    /// 用于托盘快恢菜单：点击即恢复显示。
    /// 只列 hidden：visible 的窗口已存在，failed 的恢复会再失败。
    pub fn list_recent_hidden(&self, n: usize) -> Vec<PinMeta> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut pins: Vec<PinMeta> = inner
            .values()
            .filter(|e| e.meta.state == PinState::Hidden)
            .map(|e| e.meta.clone())
            .collect();
        pins.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        pins.truncate(n);
        pins
    }

    /// 启动时从磁盘加载历史 Pin。
    /// - 读取 state.json
    /// - 对每个 meta，加载对应的 pins/{pinId}.json
    /// - doc 缺失/损坏的标记 failed，用占位 doc（避免 list() 漏掉）
    /// - 所有 visible 状态改为 hidden（应用重启后窗口实际不可见，state.json 与实际保持一致）
    /// - 有变更时持久化更新后的 state.json
    ///
    /// 不自动恢复窗口：用户从托盘快恢或管理界面主动 show。
    /// 理由：重启后突然出现一堆窗口可能打扰用户；主动恢复更可控。
    /// 满足 phase-plan.md "应用重启后历史仍在"——历史在 state.json，用户可恢复。
    pub fn load_from_disk(&self) {
        let state = storage::load_state();
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.clear();

        let now = now_iso();
        let mut changed = false;

        for mut meta in state.pins {
            // M5：校验从 state.json 读出的 pin_id 格式，防被篡改的 state.json 用于路径穿越。
            // validate_pin_id 拒绝含 / \ .. \0 的 pin_id 以及保留 label "manager"。
            if storage::validate_pin_id(&meta.pin_id).is_err() {
                eprintln!(
                    "[agent-pin] skipping pin with invalid id from state.json: {}",
                    meta.pin_id
                );
                continue;
            }

            // visible → hidden（重启后窗口实际不可见）
            if meta.state == PinState::Visible {
                meta.state = PinState::Hidden;
                meta.updated_at = now.clone();
                changed = true;
            }

            // 加载 doc
            let doc = match storage::load_pin_doc(&meta.pin_id) {
                Some(d) => d,
                None => {
                    eprintln!(
                        "[agent-pin] pins/{}.json 缺失或损坏，标记为 failed",
                        meta.pin_id
                    );
                    meta.state = PinState::Failed;
                    meta.updated_at = now.clone();
                    changed = true;
                    // 占位 doc：让管理界面能显示，用户可删除
                    PinDocument {
                        version: 1,
                        title: meta.title.clone(),
                        blocks: vec![crate::pin::PinBlock::Markdown(crate::pin::MarkdownBlock {
                            content: format!(
                                "[数据缺失] Pin 文件 {} 损坏或丢失，请删除此 Pin",
                                meta.pin_id
                            ),
                        })],
                        window: None,
                        source: meta.source.clone(),
                        created_at: Some(meta.created_at.clone()),
                    }
                }
            };

            inner.insert(meta.pin_id.clone(), PinEntry { doc, meta });
        }

        if changed {
            if let Err(e) = persist_state_locked(&inner) {
                eprintln!(
                    "[agent-pin] failed to save state.json after load_from_disk: {}",
                    e
                );
            }
        }
    }
}

pub static REGISTRY: Lazy<PinRegistry> = Lazy::new(PinRegistry::new);

/// 生成 pinId：pin_<timestamp_ms>_<6位随机>
/// 不使用 title 做 slug，避免中文/特殊字符/长度问题。
pub fn generate_pin_id() -> String {
    use rand::Rng;
    let ts = chrono::Utc::now().timestamp_millis();
    let rand6: u32 = rand::thread_rng().gen_range(0..1_000_000);
    format!("pin_{}_{:06}", ts, rand6)
}

// ---------- 内部辅助 ----------

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// 构造 PinMeta。
/// created_at 优先用 doc.createdAt（CLI 可能设置），否则用当前时间。
fn build_meta(pin_id: &str, doc: &PinDocument, state: PinState) -> PinMeta {
    let now = now_iso();
    let created_at = doc.created_at.clone().unwrap_or_else(|| now.clone());
    PinMeta {
        pin_id: pin_id.to_string(),
        title: doc.title.clone(),
        created_at,
        updated_at: now,
        state,
        source: doc.source.clone(),
    }
}

/// 从内存 entries 重新生成 state.json 并保存。
/// pins 按 createdAt 升序排列（最早的在前），查询最近时反转。
/// 调用方必须持有锁。
fn persist_state_locked(inner: &HashMap<String, PinEntry>) -> std::io::Result<()> {
    let mut pins: Vec<PinMeta> = inner.values().map(|e| e.meta.clone()).collect();
    pins.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    let state = StateFile { version: 1, pins };
    storage::save_state(&state)
}

// ---------- 测试 ----------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_pin_id_format() {
        let id = generate_pin_id();
        assert!(
            id.starts_with("pin_"),
            "pin_id must start with 'pin_': {}",
            id
        );
        let parts: Vec<&str> = id.split('_').collect();
        assert_eq!(
            parts.len(),
            3,
            "pin_id must have 3 underscore-separated parts: {}",
            id
        );
        assert_eq!(parts[0], "pin");
        // timestamp 必须可解析为 i64
        parts[1]
            .parse::<i64>()
            .expect("timestamp part must be numeric");
        // 随机部分必须 6 位数字
        assert_eq!(parts[2].len(), 6, "random part must be 6 digits: {}", id);
        parts[2]
            .parse::<u32>()
            .expect("random part must be numeric");
    }

    #[test]
    fn test_generate_pin_id_uniqueness() {
        // 连续生成 100 个 id，应全部不同
        let mut ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        for _ in 0..100 {
            ids.insert(generate_pin_id());
        }
        assert_eq!(ids.len(), 100, "generated pin_ids should be unique");
    }

    #[test]
    fn test_registry_new_empty() {
        let reg = PinRegistry::new();
        assert!(reg.list().is_empty());
        assert!(reg.list_recent_hidden(5).is_empty());
        assert!(reg.get("nonexistent").is_none());
        assert!(reg.get_meta("nonexistent").is_none());
    }

    #[test]
    fn test_build_meta_uses_doc_created_at() {
        let doc = PinDocument {
            version: 1,
            title: "Test".to_string(),
            blocks: vec![],
            window: None,
            source: None,
            created_at: Some("2026-01-01T00:00:00Z".to_string()),
        };
        let meta = build_meta("pin_123_000001", &doc, PinState::Visible);
        assert_eq!(meta.pin_id, "pin_123_000001");
        assert_eq!(meta.title, "Test");
        assert_eq!(meta.created_at, "2026-01-01T00:00:00Z");
        assert_eq!(meta.state, PinState::Visible);
    }

    #[test]
    fn test_build_meta_fallback_created_at() {
        let doc = PinDocument {
            version: 1,
            title: "Test".to_string(),
            blocks: vec![],
            window: None,
            source: None,
            created_at: None,
        };
        let meta = build_meta("pin_123_000001", &doc, PinState::Hidden);
        // created_at 应回退到当前时间（非空）
        assert!(!meta.created_at.is_empty());
        assert_eq!(meta.state, PinState::Hidden);
    }
}
