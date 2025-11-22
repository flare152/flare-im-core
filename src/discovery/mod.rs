//! 服务发现模块
//!
//! 从配置文件中自动读取服务发现配置，构建服务注册和发现实例
//!
//! ## 使用方式
//!
//! ```rust
//! use flare_im_core::discovery::init_from_app_config;
//! use std::net::SocketAddr;
//!
//! let address: SocketAddr = "127.0.0.1:8080".parse()?;
//! if let Some((registry, discover, updater)) = init_from_app_config("session", address, None).await? {
//!     // registry 会自动处理心跳续期
//!     // 当 registry 被 drop 时，会自动注销服务
//!     // 使用 discover 进行服务发现
//!     let _registry = registry;
//! }
//! ```

pub mod init;

// 统一服务发现模块已移动到 flare-server-core
// 通过 re-export 提供访问
pub use flare_server_core::discovery::{
    DiscoveryBackend, DiscoveryConfig, DiscoveryFactory, ServiceDiscover, ServiceDiscoverUpdater,
    ServiceInstance, ServiceRegistry, BackendType, LoadBalanceStrategy, NamespaceConfig, VersionConfig,
    TagFilter, InstanceMetadata, ServiceClient, ChannelService, HealthCheckConfig,
};

// Re-exports
pub use init::{
    init_from_app_config, init_from_config, init_from_registry_config,
    register_service_only, register_service_from_config, register_service_from_registry_config,
    create_discover, create_discover_from_config, create_discover_from_registry_config,
};

// 类型别名，方便使用
pub type Registry = ServiceRegistry;
pub type Discover = ServiceDiscover;
pub type Updater = ServiceDiscoverUpdater;
