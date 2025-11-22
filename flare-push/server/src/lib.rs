pub mod application;
pub mod config;
pub mod domain;
pub mod error;
// hooks 模块已从 flare-im-core 导入，不需要重新定义
// pub mod hooks;
pub mod infrastructure;
pub mod interface;
pub mod service;

pub use service::ApplicationBootstrap;
