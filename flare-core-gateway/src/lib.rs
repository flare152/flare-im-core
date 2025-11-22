//! # Flare Core Gateway
//!
//! 核心网关服务，提供统一的gRPC接口

pub mod application;
pub mod config;
pub mod domain;
pub mod error;
pub mod infrastructure;
pub mod interface;
pub mod service;

pub use config::GatewayConfig;
pub use service::ApplicationBootstrap;

