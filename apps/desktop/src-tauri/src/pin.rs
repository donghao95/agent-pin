// Pin 数据模型与校验逻辑 - re-export 自 packages/shared
//
// 类型定义和校验逻辑已抽取到 packages/shared crate（agent-pin-shared），
// desktop 后端和 CLI 共用，避免类型漂移。
// 此文件通过 wildcard re-export 保持 desktop 现有 `use crate::pin::...` 引用不变，
// 未来新增类型无需同步修改此文件。

pub use agent_pin_shared::*;
