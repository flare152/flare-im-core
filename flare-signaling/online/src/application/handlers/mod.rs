//! CQRS Handler（编排层）

pub mod command_handler;
pub mod query_handler;

pub use command_handler::OnlineCommandHandler;
pub use query_handler::OnlineQueryHandler;
