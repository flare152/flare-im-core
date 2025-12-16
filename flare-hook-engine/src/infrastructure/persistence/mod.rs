//! # Hook配置持久化
//!
//! 提供Hook配置的持久化能力

pub mod postgres_config;

pub use postgres_config::PostgresHookConfigRepository;
