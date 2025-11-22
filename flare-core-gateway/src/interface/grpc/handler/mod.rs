//! # Gateway gRPC Handler模块
//!
//! 包含所有gRPC服务处理器

pub mod access_gateway;
pub mod admin;
pub mod business;
pub mod gateway;
pub mod push;
pub mod signaling;
pub mod storage;

pub use access_gateway::{AccessGatewayHandler, GatewayRouter};
pub use gateway::GatewayHandler;
pub use business::{
    MessageServiceHandler, PushServiceHandler, SessionServiceHandler, UserServiceHandler,
};
pub use admin::{
    ConfigServiceHandler, HookServiceHandler, MetricsServiceHandler, TenantServiceHandler,
};

