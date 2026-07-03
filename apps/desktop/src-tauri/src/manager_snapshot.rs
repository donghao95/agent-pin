// Manager 列表权威快照
//
// 职责：
// - 生成 ManagerSnapshot（只读 registry，不读窗口事实，不 repair）
// - 推送给 Manager 前端，替代旧的 pins:changed → invoke list_pins 拉模型
//
// 设计原则（第一性原理）：
// - registry.state 是权威生命周期状态（系统意图），不是 window.is_visible()（实现瞬时状态）
// - AsyncCreate 过渡期由前端临时态 opening 处理，不靠 snapshot 读窗口事实
// - 异常窗口销毁由 Destroyed handler 修正 registry 后 emit snapshot，不在 snapshot build 里修
// - snapshot 是纯读视图，绝不写 registry（避免掩盖状态链路 bug）
//
// 推模型：
// - 后端状态变化后调 emit_manager_snapshot → emit_to("manager", "manager:snapshot", snapshot)
// - Manager 操作命令（hide/delete/hideAll）直接返回 snapshot
// - show_pin 例外：异步创建，命令只返回 ack，创建成功后推 snapshot
//
// 不做：
// - 不读 window.is_visible() 覆盖 registry state（会在 ESC 过渡期反向误判）
// - 不 repair（set_state_quiet 修正 registry 会掩盖链路 bug + 制造 build 竞态）
// - 不引入 revision（Tauri emit_to 同窗口保序，前端 request sequence 防 invoke 竞态即可）

use tauri::{AppHandle, Emitter, Manager};

use crate::storage::PinMeta;

/// Manager 列表权威快照。
/// payload 直接携带完整 PinMeta[]，前端收到后直接 setPins，不再二次 invoke。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ManagerSnapshot {
    pub pins: Vec<PinMeta>,
}

/// 生成 ManagerSnapshot：只读 registry，不读窗口事实，不 repair。
pub fn build() -> ManagerSnapshot {
    ManagerSnapshot {
        pins: crate::registry::REGISTRY.list(),
    }
}

/// 推送 manager:snapshot 事件给 Manager 窗口。
///
/// 若 Manager 窗口不存在（未打开或已退出），静默跳过——
/// Manager 下次 open 时会由 open_manager_window 主动拉一次 snapshot。
///
/// 注意：本函数只负责推送，不负责状态变更。调用方必须先完成 registry 状态变更。
pub fn emit_manager_snapshot(app: &AppHandle) {
    if app.get_webview_window("manager").is_none() {
        return;
    }
    let snapshot = build();
    if let Err(e) = app.emit_to("manager", "manager:snapshot", snapshot) {
        eprintln!("[agent-pin] emit manager:snapshot failed: {}", e);
    }
}
