pub mod application;
pub mod config;
pub mod domain;
pub mod error;
pub mod handler;
pub mod infrastructure;
pub mod interface;
pub mod service;
pub mod transform;

// 重新导出常用的类型
// pub use crate::interface::grpc::handler::{SimpleGatewayHandler, LightweightGatewayHandler};
pub use crate::infrastructure::database::{create_db_pool, create_db_pool_from_env};
pub use crate::service::bootstrap::ApplicationBootstrap;
