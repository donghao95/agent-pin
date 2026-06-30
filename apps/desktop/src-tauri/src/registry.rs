// Pin 内存注册表
//
// 职责：以 pinId 为 key 保存 PinDocument，供前端通过 invoke(get_pin_document) 读取。
//
// Phase 1 设计：
// - 纯内存，不持久化。关闭 Pin 窗口 = 从 registry 移除，不承诺恢复。
// - 全局单例，HTTP handler 和 Tauri invoke 共用。
// - Phase 2 会替换为文件系统存储 + 历史记录。
//
// 选型说明：用 once_cell::Lazy 全局单例而非 Tauri State，是因为 axum handler 和
// Tauri invoke 是两套 state 体系，全局单例最简单且 Phase 1 单进程无并发问题。

use std::collections::HashMap;
use std::sync::Mutex;

use once_cell::sync::Lazy;

use crate::pin::PinDocument;

pub struct PinRegistry {
    inner: Mutex<HashMap<String, PinDocument>>,
}

impl PinRegistry {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    pub fn insert(&self, pin_id: String, doc: PinDocument) {
        self.inner.lock().unwrap().insert(pin_id, doc);
    }

    pub fn get(&self, pin_id: &str) -> Option<PinDocument> {
        self.inner.lock().unwrap().get(pin_id).cloned()
    }

    pub fn remove(&self, pin_id: &str) {
        self.inner.lock().unwrap().remove(pin_id);
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
