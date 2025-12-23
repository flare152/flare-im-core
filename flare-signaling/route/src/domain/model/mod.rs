pub mod route;

pub use route::*;

// 重新导出旧的 Route 结构（用于向后兼容，逐步迁移）
#[deprecated(note = "Use Route aggregate root instead")]
pub use crate::domain::model::route::Route as RouteLegacy;

