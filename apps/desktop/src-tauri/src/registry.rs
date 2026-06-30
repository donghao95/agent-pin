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
    ///   3. 写 state.json（失败则回滚：移除内存 entry + 删 pins/{pinId}.json + 返回 Err）
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
        let mut inner = self.inner.lock().unwrap();
        inner.insert(pin_id.clone(), PinEntry {
            doc,
            meta: meta.clone(),
        });

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
    pub fn get(&self, pin_id: &str) -> Option<PinDocument> {
        self.inner
            .lock()
            .unwrap()
            .get(pin_id)
            .map(|e| e.doc.clone())
    }

    /// 读取 PinMeta。
    pub fn get_meta(&self, pin_id: &str) -> Option<PinMeta> {
        self.inner
            .lock()
            .unwrap()
            .get(pin_id)
            .map(|e| e.meta.clone())
    }

    /// 更新 Pin 状态（visible ↔ hidden ↔ failed）。
    /// 同时更新 updated_at 和 state.json。
    /// Pin 不存在时返回 Err。
    pub fn set_state(&self, pin_id: &str, state: PinState) -> Result<(), String> {
        let mut inner = self.inner.lock().unwrap();
        let entry = inner
            .get_mut(pin_id)
            .ok_or_else(|| format!("pin not found: {}", pin_id))?;
        entry.meta.state = state;
        entry.meta.updated_at = now_iso();

        if let Err(e) = persist_state_locked(&inner) {
            eprintln!(
                "[agent-pin] failed to save state.json after set_state: {}",
                e
            );
        }
        Ok(())
    }

    /// 删除 Pin（不可恢复）。
    /// 顺序：
    ///   1. 删 pins/{pinId}.json（失败返回 Err，不修改内存）
    ///   2. 从内存移除
    ///   3. 写 state.json（失败 eprintln，不回滚）
    pub fn remove(&self, pin_id: &str) -> Result<(), String> {
        // 1. 删 doc 文件
        if let Err(e) = storage::delete_pin_doc(pin_id) {
            return Err(format!("failed to delete pin doc: {}", e));
        }

        // 2. 从内存移除
        let mut inner = self.inner.lock().unwrap();
        inner.remove(pin_id);

        // 3. 写 state
        if let Err(e) = persist_state_locked(&inner) {
            eprintln!(
                "[agent-pin] failed to save state.json after remove: {}",
                e
            );
        }
        Ok(())
    }

    /// 列出所有 Pin 的 meta（按 createdAt 降序，最新的在前）。
    /// 用于管理界面完整历史列表。
    pub fn list(&self) -> Vec<PinMeta> {
        let inner = self.inner.lock().unwrap();
        let mut pins: Vec<PinMeta> = inner.values().map(|e| e.meta.clone()).collect();
        pins.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        pins
    }

    /// 列出最近 n 个 hidden Pin（按 updatedAt 降序，最近变更的在前）。
    /// 用于托盘快恢菜单：点击即恢复显示。
    /// 只列 hidden：visible 的窗口已存在，failed 的恢复会再失败。
    pub fn list_recent_hidden(&self, n: usize) -> Vec<PinMeta> {
        let inner = self.inner.lock().unwrap();
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
        let mut inner = self.inner.lock().unwrap();
        inner.clear();

        let now = now_iso();
        let mut changed = false;

        for mut meta in state.pins {
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
                        blocks: vec![crate::pin::PinBlock::Markdown(
                            crate::pin::MarkdownBlock {
                                content: format!(
                                    "[数据缺失] Pin 文件 {} 损坏或丢失，请删除此 Pin",
                                    meta.pin_id
                                ),
                            },
                        )],
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
    let state = StateFile {
        version: 1,
        pins,
    };
    storage::save_state(&state)
}
