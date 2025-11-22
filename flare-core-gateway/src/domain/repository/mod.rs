//! # 仓储层
//!
//! 提供数据访问接口，包括租户、Hook配置等数据的持久化。

pub mod tenant;
pub mod hook;

pub use tenant::{TenantRepository, TenantRepositoryImpl};
pub use hook::{HookConfigRepository, HookConfigRepositoryImpl, HookConfigRecord};

