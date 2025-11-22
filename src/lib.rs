//! Flare IM Core 公共库
//!
//! 提供统一的配置加载和服务注册发现功能

pub mod config;
pub mod discovery;
pub mod error;
pub mod gateway;
pub mod hooks;
pub mod metrics;
pub mod service_names;
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
pub use discovery::{
    init_from_app_config, init_from_config, init_from_registry_config,
    register_service_only, register_service_from_config, register_service_from_registry_config,
    // 类型别名
    Registry, Discover, Updater,
    // 重新导出 flare-server-core 的发现相关类型
    DiscoveryBackend, DiscoveryConfig, DiscoveryFactory, ServiceDiscover, ServiceDiscoverUpdater,
    ServiceInstance, ServiceRegistry, BackendType, LoadBalanceStrategy, NamespaceConfig, VersionConfig,
    TagFilter, InstanceMetadata, ServiceClient, ChannelService, HealthCheckConfig,
};

pub use gateway::{GatewayRouter, GatewayRouterConfig, GatewayRouterError, GatewayRouterTrait};
pub use service_names::{validate_service_name, service_name_env_var, get_service_name};
pub use service_names::service_names::*; // 导出所有服务名常量
pub use utils::*;
pub use tracing::init_tracing_from_config;

// Re-export helper functions (already exported via utils::*)
