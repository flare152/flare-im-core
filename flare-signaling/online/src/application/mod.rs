//! 应用服务层（Command / Query）

pub mod commands;
pub mod handlers;
pub mod queries;

pub use handlers::{OnlineCommandHandler, OnlineQueryHandler};
