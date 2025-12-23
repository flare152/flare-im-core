//! 服务模块 - 仅包含应用启动、依赖注入和服务注册
//!
//! 注意：业务逻辑已下沉到 Domain 层，此模块只负责应用启动和依赖注入

pub mod bootstrap;
pub mod metrics;
pub mod wire;

pub use bootstrap::ApplicationBootstrap;
pub use metrics::RouterMetrics;

// 向后兼容：保留 router 和 device_router 的导出（已废弃，建议使用 domain 层）
#[deprecated(note = "Use domain::value_objects and domain::service instead")]
pub mod router;

#[deprecated(note = "Use domain::entities::device_route::DeviceRoute instead")]
pub mod device_router;
