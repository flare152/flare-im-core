//! 服务模块 - 包含服务启动、注册和管理相关功能

pub mod bootstrap;
pub mod metrics;
pub mod router;
pub mod wire;
pub mod device_router;

pub use bootstrap::ApplicationBootstrap;
pub use router::{Router, RouteContext};
pub use metrics::RouterMetrics;
pub use device_router::{DeviceRouter, DeviceRouteInfo};

