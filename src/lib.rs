//! Flare IM Core 公共库
//!
//! 提供统一的配置加载和服务注册发现功能

pub mod config;
pub mod error;
pub mod hooks;
pub mod metrics;
pub mod registry;
pub mod tracing;
pub mod utils;

pub use config::{
    AccessGatewayServiceConfig, ConfigManager, FlareAppConfig, KafkaClusterConfig,
    MediaServiceConfig, MessageOrchestratorServiceConfig, MongoInstanceConfig,
    ObjectStoreConfig, PostgresInstanceConfig, RedisPoolConfig, ServiceEndpointConfig,
    ServiceRuntimeConfig, SessionPolicyConfig, SessionServiceConfig,
    SignalingOnlineServiceConfig, SignalingRouteServiceConfig, StorageReaderServiceConfig,
    StorageWriterServiceConfig, app_config, load_config, load_config_with_validation,
};
pub use error::*;
pub use hooks::*;
pub use registry::register_service;
pub use utils::*;

// Re-export helper functions (already exported via utils::*)
