//! # Hook引擎应用层
//!
//! 提供Hook执行的应用服务接口

pub mod commands;
pub mod handlers;
pub mod queries;

pub use handlers::{HookCommandHandler, HookQueryHandler};

