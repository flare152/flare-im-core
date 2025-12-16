//! Flare IM Core 公共库
//!
//! 提供统一的配置加载和服务注册发现功能

pub mod ack;
pub mod config;
pub mod discovery;
pub mod error;
pub mod gateway;
pub mod hooks;
pub mod metrics;
pub mod service_names;
pub mod tracing;
pub mod utils;

// 重新导出 ACK 相关类型（AckServiceConfig 通过 ack::AckServiceConfig 访问）
pub use ack::{
    AckEvent, AckManager, AckModule, AckStatus, AckTimeoutEvent, AckType, ImportanceLevel,
};

pub use config::{
    AccessGatewayServiceConfig, ConfigManager, FlareAppConfig, KafkaClusterConfig,
    MediaServiceConfig, MessageOrchestratorServiceConfig, MongoInstanceConfig, ObjectStoreConfig,
    PostgresInstanceConfig, RedisPoolConfig, ServiceEndpointConfig, ServiceRuntimeConfig,
    SessionPolicyConfig, SessionServiceConfig, SignalingOnlineServiceConfig,
    SignalingRouteServiceConfig, StorageReaderServiceConfig, StorageWriterServiceConfig,
    app_config, load_config, load_config_with_validation,
};
pub use discovery::{
    BackendType,
    ChannelService,
    Discover,
    // 重新导出 flare-server-core 的发现相关类型
    DiscoveryBackend,
    DiscoveryConfig,
    DiscoveryFactory,
    HealthCheckConfig,
    InstanceMetadata,
    LoadBalanceStrategy,
    NamespaceConfig,
    // 类型别名
    Registry,
    ServiceClient,
    ServiceDiscover,
    ServiceDiscoverUpdater,
    ServiceInstance,
    ServiceRegistry,
    TagFilter,
    Updater,
    VersionConfig,
    init_from_app_config,
    init_from_config,
    init_from_registry_config,
    register_service_from_config,
    register_service_from_registry_config,
    register_service_only,
};
pub use error::*;
pub use hooks::*;

pub use gateway::{GatewayRouter, GatewayRouterConfig, GatewayRouterError, GatewayRouterTrait};
pub use service_names::service_names::*; // 导出所有服务名常量
pub use service_names::{get_service_name, service_name_env_var, validate_service_name};
pub use tracing::init_tracing_from_config;
pub use utils::*;

// Re-export helper functions (already exported via utils::*)
