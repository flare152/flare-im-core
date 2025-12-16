use std::env;

use flare_im_core::config::FlareAppConfig;
use flare_server_core::kafka::KafkaProducerConfig;

use crate::domain::model::MessageDefaults;

#[derive(Clone, Debug)]
pub struct MessageOrchestratorConfig {
    pub kafka_bootstrap: String,
    pub kafka_storage_topic: String, // 存储队列: flare.im.message.created
    pub kafka_push_topic: String,    // 推送队列: flare.im.push.tasks
    pub kafka_timeout_ms: u64,
    // 批量发送配置
    pub kafka_batch_size: usize,      // 批量发送大小
    pub kafka_flush_interval_ms: u64, // 刷新间隔（毫秒）
    pub redis_url: Option<String>,
    pub wal_hash_key: Option<String>,
    pub wal_ttl_seconds: u64,
    pub default_tenant_id: Option<String>,
    pub default_business_type: String,
    pub default_session_type: String,
    pub default_sender_type: String,
    pub reader_endpoint: Option<String>,
    pub hook_config: Option<String>,
    pub hook_config_dir: Option<String>,
    pub session_service_type: Option<String>,
}

fn env_or_fallback(primary: &str, fallback: &str) -> Option<String> {
    env::var(primary).ok().or_else(|| env::var(fallback).ok())
}

impl MessageOrchestratorConfig {
    pub fn from_sources(app: Option<&FlareAppConfig>) -> Self {
        let (service_config, kafka_profile, redis_profile) = if let Some(cfg) = app {
            let svc = cfg.message_orchestrator_service();
            let kafka_profile = svc
                .kafka
                .as_deref()
                .and_then(|name| cfg.kafka_profile(name))
                .cloned();
            let redis_profile = svc
                .wal_store
                .as_deref()
                .and_then(|name| cfg.redis_profile(name))
                .cloned();
            (Some(svc), kafka_profile, redis_profile)
        } else {
            (None, None, None)
        };

        let kafka_bootstrap = env_or_fallback(
            "MESSAGE_ORCHESTRATOR_KAFKA_BOOTSTRAP",
            "STORAGE_KAFKA_BOOTSTRAP_SERVERS",
        )
        .or_else(|| {
            kafka_profile
                .as_ref()
                .map(|profile| profile.bootstrap_servers.clone())
        })
        .unwrap_or_else(|| "127.0.0.1:29092".to_string());

        let kafka_storage_topic = env_or_fallback(
            "MESSAGE_ORCHESTRATOR_KAFKA_STORAGE_TOPIC",
            "STORAGE_KAFKA_STORAGE_TOPIC",
        )
        .or_else(|| {
            service_config
                .as_ref()
                .and_then(|service| service.kafka_topic.clone())
        })
        .unwrap_or_else(|| "flare.im.message.created".to_string());

        let kafka_push_topic = env_or_fallback(
            "MESSAGE_ORCHESTRATOR_KAFKA_PUSH_TOPIC",
            "PUSH_KAFKA_PUSH_TOPIC",
        )
        .unwrap_or_else(|| "flare.im.push.tasks".to_string());

        let kafka_timeout_ms = env_or_fallback(
            "MESSAGE_ORCHESTRATOR_KAFKA_TIMEOUT_MS",
            "STORAGE_KAFKA_TIMEOUT_MS",
        )
        .and_then(|v| v.parse::<u64>().ok())
        .or_else(|| {
            kafka_profile
                .as_ref()
                .and_then(|profile| profile.timeout_ms)
        })
        .unwrap_or(5000);

        // 批量发送配置
        let kafka_batch_size =
            env_or_fallback("MESSAGE_ORCHESTRATOR_KAFKA_BATCH_SIZE", "KAFKA_BATCH_SIZE")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(100); // 默认批量大小：100

        let kafka_flush_interval_ms = env_or_fallback(
            "MESSAGE_ORCHESTRATOR_KAFKA_FLUSH_INTERVAL_MS",
            "KAFKA_FLUSH_INTERVAL_MS",
        )
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(50); // 默认刷新间隔：50ms

        let redis_url = env_or_fallback("MESSAGE_ORCHESTRATOR_REDIS_URL", "STORAGE_REDIS_URL")
            .or_else(|| redis_profile.as_ref().map(|profile| profile.url.clone()));

        let wal_hash_key =
            env_or_fallback("MESSAGE_ORCHESTRATOR_WAL_HASH_KEY", "STORAGE_WAL_HASH_KEY")
                .or_else(|| {
                    service_config
                        .as_ref()
                        .and_then(|service| service.wal_hash_key.clone())
                })
                .or_else(|| redis_url.as_ref().map(|_| "storage:wal:buffer".to_string()));

        let wal_ttl_seconds = env_or_fallback(
            "MESSAGE_ORCHESTRATOR_WAL_TTL_SECONDS",
            "STORAGE_WAL_TTL_SECONDS",
        )
        .and_then(|v| v.parse::<u64>().ok())
        .or_else(|| {
            service_config
                .as_ref()
                .and_then(|service| service.wal_ttl_seconds)
        })
        .or_else(|| {
            redis_profile
                .as_ref()
                .and_then(|profile| profile.ttl_seconds)
        })
        .unwrap_or(24 * 3600);

        let default_tenant_id = env_or_fallback(
            "MESSAGE_ORCHESTRATOR_DEFAULT_TENANT_ID",
            "STORAGE_DEFAULT_TENANT_ID",
        );

        let default_business_type = env_or_fallback(
            "MESSAGE_ORCHESTRATOR_DEFAULT_BUSINESS_TYPE",
            "STORAGE_DEFAULT_BUSINESS_TYPE",
        )
        .unwrap_or_else(|| "im".to_string());

        let default_session_type = env_or_fallback(
            "MESSAGE_ORCHESTRATOR_DEFAULT_SESSION_TYPE",
            "STORAGE_DEFAULT_SESSION_TYPE",
        )
        .unwrap_or_else(|| "single".to_string());

        let default_sender_type = env_or_fallback(
            "MESSAGE_ORCHESTRATOR_DEFAULT_SENDER_TYPE",
            "STORAGE_DEFAULT_SENDER_TYPE",
        )
        .unwrap_or_else(|| "user".to_string());

        let reader_endpoint = env_or_fallback(
            "MESSAGE_ORCHESTRATOR_READER_ENDPOINT",
            "STORAGE_READER_ENDPOINT",
        );

        let hook_config =
            env_or_fallback("MESSAGE_ORCHESTRATOR_HOOKS_CONFIG", "STORAGE_HOOKS_CONFIG").or_else(
                || {
                    service_config
                        .as_ref()
                        .and_then(|service| service.hook_config.clone())
                },
            );

        let hook_config_dir = env_or_fallback(
            "MESSAGE_ORCHESTRATOR_HOOKS_CONFIG_DIR",
            "STORAGE_HOOKS_CONFIG_DIR",
        )
        .or_else(|| {
            service_config
                .as_ref()
                .and_then(|service| service.hook_config_dir.clone())
        });

        // 从配置中获取 session_service_type
        let session_service_type = service_config
            .as_ref()
            .and_then(|service| service.session_service_type.clone())
            .or_else(|| env::var("MESSAGE_ORCHESTRATOR_SESSION_SERVICE_TYPE").ok());

        Self {
            kafka_bootstrap,
            kafka_storage_topic,
            kafka_push_topic,
            kafka_timeout_ms,
            kafka_batch_size,
            kafka_flush_interval_ms,
            redis_url,
            wal_hash_key,
            wal_ttl_seconds,
            default_tenant_id,
            default_business_type,
            default_session_type,
            default_sender_type,
            reader_endpoint,
            hook_config,
            hook_config_dir,
            session_service_type,
        }
    }

    /// 从应用配置加载（新方式，推荐）
    pub fn from_app_config(app: &FlareAppConfig) -> Self {
        Self::from_sources(Some(app))
    }

    /// 从环境变量加载（保留用于向后兼容，但不推荐使用）
    #[deprecated(note = "Use from_app_config instead")]
    pub fn from_env() -> Self {
        Self::from_sources(None)
    }

    pub fn defaults(&self) -> MessageDefaults {
        MessageDefaults {
            default_business_type: self.default_business_type.clone(),
            default_session_type: self.default_session_type.clone(),
            default_sender_type: self.default_sender_type.clone(),
            default_tenant_id: self.default_tenant_id.clone(),
        }
    }
}

// 实现 KafkaProducerConfig trait，使 MessageOrchestratorConfig 可以使用通用的 Kafka 生产者构建器
impl KafkaProducerConfig for MessageOrchestratorConfig {
    fn kafka_bootstrap(&self) -> &str {
        &self.kafka_bootstrap
    }

    fn message_timeout_ms(&self) -> u64 {
        self.kafka_timeout_ms
    }

    // 使用默认值，或根据需要覆盖
    fn enable_idempotence(&self) -> bool {
        true // 消息编排器需要保证消息不丢失
    }

    fn compression_type(&self) -> &str {
        "snappy" // 使用 snappy 压缩，平衡性能和压缩比
    }
}
