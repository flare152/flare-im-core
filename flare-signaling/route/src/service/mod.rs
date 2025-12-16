//! 服务模块 - 包含服务启动、注册和管理相关功能

pub mod bootstrap;
pub mod device_router;
pub mod metrics;
pub mod router;
pub mod wire;

pub use bootstrap::ApplicationBootstrap;
pub use device_router::{DeviceRouteInfo, DeviceRouter};
pub use metrics::RouterMetrics;
pub use router::{RouteContext, Router};
