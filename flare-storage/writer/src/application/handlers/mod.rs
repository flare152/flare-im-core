//! CQRS Handler（编排层）

pub mod command_handler;

pub use command_handler::MessagePersistenceCommandHandler;
