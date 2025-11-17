//! # Hook引擎gRPC接口
//!
//! 提供Hook引擎的gRPC服务接口

pub mod server;
pub mod hook_service;

pub use server::HookExtensionServer;
pub use hook_service::HookServiceServer;

