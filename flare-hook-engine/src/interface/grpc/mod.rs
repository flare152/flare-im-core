//! # Hook引擎gRPC接口
//!
//! 提供Hook引擎的gRPC服务接口

pub mod hook_service;
pub mod server;

pub use hook_service::HookServiceServer;
pub use server::HookExtensionServer;
