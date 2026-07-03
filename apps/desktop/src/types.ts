// Agent Pin 前端共享类型定义
// 与后端 packages/shared/src/lib.rs 对齐
// 集中 Pin.tsx 和 Manager.tsx 共用的类型，避免类型漂移（M21）

// ---------- Window / Source ----------

export type PinWindowConfig = {
  width?: number;
  // 后端 PinHeight 是 untagged enum：number | "auto"，前端类型需与之对齐（m-A4）
  height?: number | string;
  x?: number;
  y?: number;
  alwaysOnTop?: boolean;
};

export type PinSource = {
  agent?: string;
  workspace?: string;
  task?: string;
  conversationId?: string;
};

// ---------- Block ----------

// PinBlock 使用 internally tagged union（tag = "type"），与后端 serde tagged enum 对齐。
// JSON 形如 {"type":"markdown","content":"..."}。
export type PinBlock =
  | { type: "markdown"; content: string }
  | { type: "image"; path: string; caption?: string }
  | { type: "status"; level?: StatusLevel; text: string };

// ---------- PinDocument ----------

export type PinDocument = {
  version: 1;
  title: string;
  blocks: PinBlock[];
  window?: PinWindowConfig;
  source?: PinSource;
  createdAt?: string;
};

// ---------- Status ----------

export type StatusLevel = "info" | "success" | "warning" | "error";

// ---------- Pin 状态与元信息（与后端 storage.rs 对齐）----------

export type PinState = "visible" | "hidden" | "failed";

export type PinMeta = {
  pinId: string;
  title: string;
  createdAt: string;
  updatedAt: string;
  state: PinState;
  source?: PinSource;
};

// ---------- Manager 快照（后端 manager_snapshot.rs 对齐）----------

/// Manager 列表权威快照。后端状态变化后推送，前端直接 setPins，不再二次 invoke。
export type ManagerSnapshot = {
  pins: PinMeta[];
};

/// Manager 前端临时操作态（不持久化，仅 UI 表达过渡过程）。
/// - opening：用户点了"显示"，窗口异步创建中
/// - closing：用户点了"隐藏"，窗口销毁中
/// - deleting：用户点了"删除"，registry 移除中
export type TransientState = "opening" | "closing" | "deleting";
