//! Flare Signaling Common
//! 
//! 信令服务共享代码库,包含:
//! - 共享数据模型 (models)
//! - 共享错误类型 (error)
//! - 共享工具函数 (utils)
//! 
//! 被 gateway、online、route 三个子模块共同使用

pub mod error;
pub mod models;
pub mod utils;

// 导出常用类型
pub use error::{SignalingError, SignalingResult};
pub use models::*;
