// Handler模块用于处理具体的业务逻辑
// 目前业务逻辑主要在router中实现，这里预留扩展空间

pub mod access_gateway;
pub mod admin;
pub mod business;
pub mod gateway;
pub mod push;
pub mod signaling;
pub mod storage;

pub use gateway::GatewayHandler;
pub use access_gateway::{AccessGatewayHandler, GatewayRouter};
pub use business::{
    MessageServiceHandler, PushServiceHandler, SessionServiceHandler, UserServiceHandler,
};
pub use admin::{
    ConfigServiceHandler, HookServiceHandler, MetricsServiceHandler, TenantServiceHandler,
};
