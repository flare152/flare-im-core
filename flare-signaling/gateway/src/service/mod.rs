//! 服务模块 - 包含服务启动、注册和管理相关功能

pub mod bootstrap;
pub mod service_manager;
pub mod startup;
mod wire;

pub use bootstrap::ApplicationBootstrap;
pub use wire::{ApplicationContext, GrpcServices};
pub use service_manager::{PortConfig, ServiceManager};
pub use startup::{StartupInfo, GrpcServiceInfo};

