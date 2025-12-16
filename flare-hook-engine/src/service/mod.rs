//! # Hook引擎服务层
//!
//! 提供应用启动和依赖注入

pub mod bootstrap;
pub mod registry;
mod wire;

pub use bootstrap::ApplicationBootstrap;
pub use wire::ApplicationContext;
