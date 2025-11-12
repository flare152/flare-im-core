//! Flare IM Core 公共库
//!
//! 提供统一的配置加载和服务注册发现功能

pub mod config;
pub mod error;
pub mod hooks;
pub mod registry;

pub use config::{
    AccessGatewayServiceConfig, ConfigManager, FlareAppConfig, KafkaClusterConfig,
    MediaServiceConfig, MongoInstanceConfig, ObjectStoreConfig, PostgresInstanceConfig,
    RedisPoolConfig, ServiceEndpointConfig, ServiceRuntimeConfig, app_config, load_config,
};
pub use error::*;
pub use hooks::*;
pub use registry::register_service;
