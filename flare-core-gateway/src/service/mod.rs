//! # Gateway服务层
//!
//! 提供Gateway的应用启动和依赖注入

pub mod bootstrap;
mod wire;

pub use bootstrap::ApplicationBootstrap;
pub use wire::ApplicationContext;

