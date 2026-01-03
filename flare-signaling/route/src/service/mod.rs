//! 服务模块 - 仅包含应用启动、依赖注入和服务注册
//!
//! 注意：业务逻辑已下沉到 Domain 层，此模块只负责应用启动和依赖注入

pub mod bootstrap;
pub mod metrics;
pub mod wire;

pub use bootstrap::ApplicationBootstrap;
pub use metrics::RouterMetrics;
