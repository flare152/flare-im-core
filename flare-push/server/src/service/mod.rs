//! 服务模块 - 包含服务启动、注册和管理相关功能

pub mod bootstrap;
pub mod registry;

pub use bootstrap::ApplicationBootstrap;
pub use registry::ServiceRegistrar;

