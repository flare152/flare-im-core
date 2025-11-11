//! Flare IM Core 配置模块
//!
//! 该模块提供了完整的应用程序配置管理功能，包括：
//! - 配置文件加载和解析
//! - 环境特定配置覆盖
//! - 各种服务配置定义
//! - 对象存储、数据库、消息队列等基础设施配置

// 首先导入需要的模块和类型
use std::collections::HashMap;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use flare_server_core::{Config, RegistryConfig};
use serde::Deserialize;
use std::sync::OnceLock;
use toml::Value;
use tracing::warn;

// 导入配置管理器模块
mod manager;
pub use manager::ConfigManager;

/// 全局应用配置实例，使用 OnceLock 确保只初始化一次
static APP_CONFIG: OnceLock<FlareAppConfig> = OnceLock::new();

/// Redis 连接池配置
#[derive(Debug, Clone, Deserialize, Default)]
pub struct RedisPoolConfig {
    /// Redis 服务器地址
    pub url: String,
    /// 命名空间前缀
    #[serde(default)]
    pub namespace: Option<String>,
    /// 数据库编号
    #[serde(default)]
    pub database: Option<u32>,
    /// 过期时间（秒）
    #[serde(default)]
    pub ttl_seconds: Option<u64>,
}

/// Kafka 集群配置
#[derive(Debug, Clone, Deserialize, Default)]
pub struct KafkaClusterConfig {
    /// Kafka 服务器地址列表
    pub bootstrap_servers: String,
    /// 客户端标识
    #[serde(default)]
    pub client_id: Option<String>,
    /// 安全协议
    #[serde(default)]
    pub security_protocol: Option<String>,
    /// SASL 用户名
    #[serde(default)]
    pub sasl_username: Option<String>,
    /// SASL 密码
    #[serde(default)]
    pub sasl_password: Option<String>,
    /// 超时时间（毫秒）
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    /// 其他选项
    #[serde(default)]
    pub options: HashMap<String, String>,
}

/// PostgreSQL 数据库实例配置
#[derive(Debug, Clone, Deserialize, Default)]
pub struct PostgresInstanceConfig {
    /// 数据库连接 URL
    pub url: String,
    /// 最大连接数
    #[serde(default)]
    pub max_connections: Option<u32>,
    /// 最小连接数
    #[serde(default)]
    pub min_connections: Option<u32>,
}

/// MongoDB 实例配置
#[derive(Debug, Clone, Deserialize, Default)]
pub struct MongoInstanceConfig {
    /// MongoDB 连接 URL
    pub url: String,
    /// 数据库名称
    #[serde(default)]
    pub database: Option<String>,
}

/// 对象存储配置
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ObjectStoreConfig {
    /// 存储类型（如 minio, s3, oss 等）
    pub profile_type: String,
    /// 存储服务端点
    #[serde(default)]
    pub endpoint: Option<String>,
    /// 访问密钥
    #[serde(default)]
    pub access_key: Option<String>,
    /// 秘密密钥
    #[serde(default)]
    pub secret_key: Option<String>,
    /// 存储桶名称
    #[serde(default)]
    pub bucket: Option<String>,
    /// 区域
    #[serde(default)]
    pub region: Option<String>,
    /// 是否使用 SSL
    #[serde(default)]
    pub use_ssl: Option<bool>,
    /// CDN 基础 URL
    #[serde(default)]
    pub cdn_base_url: Option<String>,
    /// 上传路径前缀
    #[serde(default)]
    pub upload_prefix: Option<String>,
}

/// 服务端点配置
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ServiceEndpointConfig {
    /// 服务地址
    pub address: Option<String>,
    /// 服务端口
    pub port: Option<u16>,
}

/// 服务运行时配置
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ServiceRuntimeConfig {
    /// 服务名称
    #[serde(default)]
    pub service_name: Option<String>,
    /// 服务器配置
    #[serde(default)]
    pub server: Option<ServiceEndpointConfig>,
    /// 注册中心配置
    #[serde(default)]
    pub registry: Option<RegistryConfig>,
}

/// 接入网关服务配置
#[derive(Debug, Clone, Deserialize, Default)]
pub struct AccessGatewayServiceConfig {
    /// 运行时配置
    #[serde(flatten)]
    pub runtime: ServiceRuntimeConfig,
    /// 信令端点
    #[serde(default)]
    pub signaling_endpoint: Option<String>,
    /// 消息端点
    #[serde(default, alias = "storage_endpoint")]
    pub message_endpoint: Option<String>,
    /// 推送端点
    #[serde(default)]
    pub push_endpoint: Option<String>,
    /// 令牌密钥
    #[serde(default)]
    pub token_secret: Option<String>,
    /// 令牌发行方
    #[serde(default)]
    pub token_issuer: Option<String>,
    /// 令牌过期时间（秒）
    #[serde(default)]
    pub token_ttl_seconds: Option<u64>,
    /// 令牌存储
    #[serde(default)]
    pub token_store: Option<String>,
    /// 会话存储
    #[serde(default)]
    pub session_store: Option<String>,
    /// 会话存储过期时间（秒）
    #[serde(default)]
    pub session_store_ttl_seconds: Option<u64>,
}

/// 媒体服务配置
#[derive(Debug, Clone, Deserialize, Default)]
pub struct MediaServiceConfig {
    /// 运行时配置
    #[serde(flatten)]
    pub runtime: ServiceRuntimeConfig,
    /// 元数据存储
    #[serde(default)]
    pub metadata_store: Option<String>,
    /// 元数据缓存
    #[serde(default)]
    pub metadata_cache: Option<String>,
    /// 对象存储配置
    #[serde(default)]
    pub object_store: Option<String>,
    /// Redis 过期时间（秒）
    #[serde(default)]
    pub redis_ttl_seconds: Option<i64>,
    /// 本地存储目录
    #[serde(default)]
    pub local_storage_dir: Option<String>,
    /// 本地基础 URL
    #[serde(default)]
    pub local_base_url: Option<String>,
    /// CDN 基础 URL
    #[serde(default)]
    pub cdn_base_url: Option<String>,
    /// 孤立文件宽限时间（秒）
    #[serde(default)]
    pub orphan_grace_seconds: Option<i64>,
    /// 上传会话存储
    #[serde(default)]
    pub upload_session_store: Option<String>,
    /// 分块上传目录
    #[serde(default)]
    pub chunk_upload_dir: Option<String>,
    /// 分块过期时间（秒）
    #[serde(default)]
    pub chunk_ttl_seconds: Option<i64>,
    /// 最大分块大小（字节）
    #[serde(default)]
    pub max_chunk_size_bytes: Option<i64>,
}

/// 推送代理服务配置
#[derive(Debug, Clone, Deserialize, Default)]
pub struct PushProxyServiceConfig {
    /// 运行时配置
    #[serde(flatten)]
    pub runtime: ServiceRuntimeConfig,
    /// Kafka 配置
    #[serde(default)]
    pub kafka: Option<String>,
    /// 消息主题
    #[serde(default)]
    pub message_topic: Option<String>,
    /// 通知主题
    #[serde(default)]
    pub notification_topic: Option<String>,
    /// 超时时间（毫秒）
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

/// 推送服务器服务配置
#[derive(Debug, Clone, Deserialize, Default)]
pub struct PushServerServiceConfig {
    /// 运行时配置
    #[serde(flatten)]
    pub runtime: ServiceRuntimeConfig,
    /// Kafka 配置
    #[serde(default)]
    pub kafka: Option<String>,
    /// 消费者组
    #[serde(default)]
    pub consumer_group: Option<String>,
    /// 消息主题
    #[serde(default)]
    pub message_topic: Option<String>,
    /// 通知主题
    #[serde(default)]
    pub notification_topic: Option<String>,
    /// 任务主题
    #[serde(default)]
    pub task_topic: Option<String>,
    /// Redis 配置
    #[serde(default)]
    pub redis: Option<String>,
    /// 在线状态过期时间（秒）
    #[serde(default)]
    pub online_ttl_seconds: Option<u64>,
    /// 默认租户 ID
    #[serde(default)]
    pub default_tenant_id: Option<String>,
    /// Hook 配置
    #[serde(default)]
    pub hook_config: Option<String>,
    /// Hook 配置目录
    #[serde(default)]
    pub hook_config_dir: Option<String>,
}

/// 推送工作服务配置
#[derive(Debug, Clone, Deserialize, Default)]
pub struct PushWorkerServiceConfig {
    /// 运行时配置
    #[serde(flatten)]
    pub runtime: ServiceRuntimeConfig,
    /// Kafka 配置
    #[serde(default)]
    pub kafka: Option<String>,
    /// 消费者组
    #[serde(default)]
    pub consumer_group: Option<String>,
    /// 任务主题
    #[serde(default)]
    pub task_topic: Option<String>,
    /// 信令端点
    #[serde(default)]
    pub signaling_endpoint: Option<String>,
    /// 离线提供者
    #[serde(default)]
    pub offline_provider: Option<String>,
    /// Hook 配置
    #[serde(default)]
    pub hook_config: Option<String>,
    /// Hook 配置目录
    #[serde(default)]
    pub hook_config_dir: Option<String>,
}

/// 消息编排服务配置
#[derive(Debug, Clone, Deserialize, Default)]
pub struct MessageOrchestratorServiceConfig {
    /// Kafka 配置
    #[serde(default)]
    pub kafka: Option<String>,
    /// Kafka 主题
    #[serde(default)]
    pub kafka_topic: Option<String>,
    /// WAL 存储
    #[serde(default)]
    pub wal_store: Option<String>,
    /// WAL 哈希键
    #[serde(default)]
    pub wal_hash_key: Option<String>,
    /// WAL 过期时间（秒）
    #[serde(default)]
    pub wal_ttl_seconds: Option<u64>,
    /// Hook 配置
    #[serde(default)]
    pub hook_config: Option<String>,
    /// Hook 配置目录
    #[serde(default)]
    pub hook_config_dir: Option<String>,
}

/// Flare 应用配置主结构体
#[derive(Debug, Clone, Deserialize)]
pub struct FlareAppConfig {
    /// 核心配置
    #[serde(flatten)]
    pub core: Config,
    /// Redis 配置映射
    #[serde(default)]
    pub redis: HashMap<String, RedisPoolConfig>,
    /// Kafka 配置映射
    #[serde(default)]
    pub kafka: HashMap<String, KafkaClusterConfig>,
    /// PostgreSQL 配置映射
    #[serde(default)]
    pub postgres: HashMap<String, PostgresInstanceConfig>,
    /// MongoDB 配置映射
    #[serde(default)]
    pub mongodb: HashMap<String, MongoInstanceConfig>,
    /// 对象存储配置映射
    #[serde(default)]
    pub object_storage: HashMap<String, ObjectStoreConfig>,
    /// 服务配置
    #[serde(default)]
    pub services: ServicesConfig,
}

impl FlareAppConfig {
    /// 获取核心配置
    pub fn base(&self) -> &Config {
        &self.core
    }

    /// 获取 Redis 配置
    pub fn redis_profile(&self, name: &str) -> Option<&RedisPoolConfig> {
        self.redis.get(name)
    }

    /// 获取 Kafka 配置
    pub fn kafka_profile(&self, name: &str) -> Option<&KafkaClusterConfig> {
        self.kafka.get(name)
    }

    /// 获取 PostgreSQL 配置
    pub fn postgres_profile(&self, name: &str) -> Option<&PostgresInstanceConfig> {
        self.postgres.get(name)
    }

    /// 获取 MongoDB 配置
    pub fn mongodb_profile(&self, name: &str) -> Option<&MongoInstanceConfig> {
        self.mongodb.get(name)
    }

    /// 获取对象存储配置
    pub fn object_store_profile(&self, name: &str) -> Option<&ObjectStoreConfig> {
        self.object_storage.get(name)
    }

    /// 获取接入网关服务配置
    pub fn access_gateway_service(&self) -> AccessGatewayServiceConfig {
        self.services.access_gateway.clone().unwrap_or_default()
    }

    /// 获取媒体服务配置
    pub fn media_service(&self) -> MediaServiceConfig {
        self.services.media.clone().unwrap_or_default()
    }

    /// 获取推送代理服务配置
    pub fn push_proxy_service(&self) -> PushProxyServiceConfig {
        self.services.push_proxy.clone().unwrap_or_default()
    }

    /// 获取推送服务器服务配置
    pub fn push_server_service(&self) -> PushServerServiceConfig {
        self.services.push_server.clone().unwrap_or_default()
    }

    /// 获取推送工作服务配置
    pub fn push_worker_service(&self) -> PushWorkerServiceConfig {
        self.services.push_worker.clone().unwrap_or_default()
    }

    /// 获取消息编排服务配置
    pub fn message_orchestrator_service(&self) -> MessageOrchestratorServiceConfig {
        self.services
            .message_orchestrator
            .clone()
            .unwrap_or_default()
    }

    /// 组合服务配置
    pub fn compose_service_config(
        &self,
        runtime: &ServiceRuntimeConfig,
        fallback_name: &str,
    ) -> Config {
        let mut cfg = self.core.clone();
        cfg.service.name = runtime
            .service_name
            .as_ref()
            .cloned()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| fallback_name.to_string());

        if let Some(server) = runtime.server.as_ref() {
            if let Some(address) = server.address.as_ref() {
                cfg.server.address = address.clone();
            }
            if let Some(port) = server.port {
                cfg.server.port = port;
            }
        }

        if let Some(registry) = runtime.registry.as_ref() {
            cfg.registry = Some(registry.clone());
        }

        cfg
    }

    /// 确保配置有默认值
    fn ensure_defaults(&mut self) {
        if self.core.server.address.is_empty() {
            self.core.server.address = "0.0.0.0".to_string();
        }
        if self.core.server.port == 0 {
            self.core.server.port = 50051;
        }
    }
}

/// 加载配置
pub fn load_config(path: Option<&str>) -> &'static FlareAppConfig {
    let candidates: Vec<PathBuf> = match path {
        Some(p) => vec![PathBuf::from(p)],
        None => vec![PathBuf::from("config"), PathBuf::from("config.toml")],
    };

    APP_CONFIG.get_or_init(|| {
        let mut cfg = load_with_fallback(&candidates);
        // 加载环境特定配置
        if let Err(e) = manager::ConfigManager::load_environment_config(&mut cfg) {
            warn!("failed to load environment config: {}", e);
        }
        cfg
    })
}

/// 获取应用配置
pub fn app_config() -> &'static FlareAppConfig {
    APP_CONFIG.get().expect("configuration not initialised")
}

/// 使用备选方案加载配置
fn load_with_fallback(candidates: &[PathBuf]) -> FlareAppConfig {
    for path in candidates {
        match load_config_from_source(path) {
            Ok(mut cfg) => {
                cfg.ensure_defaults();
                return cfg;
            }
            Err(err) => {
                warn!("failed to load config from {}: {err}", path.display());
            }
        }
    }

    warn!("no configuration source succeeded, falling back to defaults");
    default_config()
}

/// 从源加载配置
fn load_config_from_source(path: &Path) -> Result<FlareAppConfig> {
    if !path.exists() {
        return Err(anyhow!(
            "configuration path {} does not exist",
            path.display()
        ));
    }

    let metadata = path
        .metadata()
        .with_context(|| format!("unable to read metadata for {}", path.display()))?;

    if metadata.is_dir() {
        load_config_from_directory(path)
    } else {
        load_config_from_file(path)
    }
}

/// 从文件加载配置
fn load_config_from_file(path: &Path) -> Result<FlareAppConfig> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("unable to read config file: {}", Path::new(path).display()))?;
    let mut cfg: FlareAppConfig = toml::from_str(&content)
        .with_context(|| format!("invalid config format: {}", Path::new(path).display()))?;
    cfg.ensure_defaults();
    Ok(cfg)
}

/// 从目录加载配置
fn load_config_from_directory(path: &Path) -> Result<FlareAppConfig> {
    let base_file = path.join("base.toml");
    if !base_file.exists() {
        return Err(anyhow!(
            "missing base configuration: {}",
            base_file.display()
        ));
    }

    let mut merged = load_toml_value(&base_file)?;

    if !merged.is_table() {
        return Err(anyhow!(
            "base configuration must be a table: {}",
            base_file.display()
        ));
    }

    merge_directory(&mut merged, &path.join("shared"))?;
    merge_directory(&mut merged, &path.join("services"))?;
    merge_directory(&mut merged, &path.join("overrides"))?;

    let cfg: FlareAppConfig = merged
        .try_into()
        .with_context(|| format!("invalid configuration after merging {}", path.display()))?;

    Ok(cfg)
}

/// 合并目录中的配置
fn merge_directory(root: &mut Value, dir: &Path) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }

    let mut entries = fs::read_dir(dir)
        .with_context(|| format!("unable to read config directory {}", dir.display()))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .path()
                .extension()
                .and_then(OsStr::to_str)
                .map(|ext| ext.eq_ignore_ascii_case("toml"))
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();

    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let value = load_toml_value(&entry.path())?;
        merge_value(root, value);
    }

    Ok(())
}

/// 加载 TOML 值
fn load_toml_value(path: &Path) -> Result<Value> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("unable to read config fragment {}", path.display()))?;
    let value: Value = toml::from_str(&content)
        .with_context(|| format!("invalid TOML content in fragment {}", path.display()))?;
    Ok(value)
}

/// 合并值
fn merge_value(base: &mut Value, overlay: Value) {
    match overlay {
        Value::Table(overlay_table) => {
            if let Value::Table(base_table) = base {
                for (key, overlay_value) in overlay_table.into_iter() {
                    match base_table.get_mut(&key) {
                        Some(base_value) => merge_value(base_value, overlay_value),
                        None => {
                            base_table.insert(key, overlay_value);
                        }
                    }
                }
            } else {
                *base = Value::Table(overlay_table);
            }
        }
        other => {
            *base = other;
        }
    }
}

/// 默认配置
fn default_config() -> FlareAppConfig {
    FlareAppConfig {
        core: Config {
            service: flare_server_core::ServiceConfig {
                name: "flare-im-core".to_string(),
                version: "0.1.0".to_string(),
            },
            server: flare_server_core::ServerConfig {
                address: "0.0.0.0".to_string(),
                port: 50051,
            },
            registry: Some(flare_server_core::RegistryConfig {
                registry_type: "etcd".to_string(),
                endpoints: vec!["http://localhost:2379".to_string()],
                namespace: "flare".to_string(),
                ttl: 30,
            }),
            mesh: None,
            storage: None,
        },
        redis: HashMap::new(),
        kafka: HashMap::new(),
        postgres: HashMap::new(),
        mongodb: HashMap::new(),
        object_storage: HashMap::new(),
        services: ServicesConfig::default(),
    }
}

/// 服务配置集合
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ServicesConfig {
    /// 接入网关服务配置
    #[serde(default)]
    pub access_gateway: Option<AccessGatewayServiceConfig>,
    /// 媒体服务配置
    #[serde(default)]
    pub media: Option<MediaServiceConfig>,
    /// 推送代理服务配置
    #[serde(default)]
    pub push_proxy: Option<PushProxyServiceConfig>,
    /// 推送服务器服务配置
    #[serde(default)]
    pub push_server: Option<PushServerServiceConfig>,
    /// 推送工作服务配置
    #[serde(default)]
    pub push_worker: Option<PushWorkerServiceConfig>,
    /// 消息编排服务配置
    #[serde(default)]
    pub message_orchestrator: Option<MessageOrchestratorServiceConfig>,
}