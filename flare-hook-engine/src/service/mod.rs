//! # Hook引擎服务层
//!
//! 提供应用启动和依赖注入

pub mod bootstrap;
pub mod registry;

pub use bootstrap::ApplicationBootstrap;

