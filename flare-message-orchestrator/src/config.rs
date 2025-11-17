use std::env;

use flare_im_core::config::FlareAppConfig;

use crate::domain::message_submission::MessageDefaults;

#[derive(Clone, Debug)]
pub struct MessageOrchestratorConfig {
    pub kafka_bootstrap: String,
    pub kafka_storage_topic: String,  // 存储队列: flare.im.message.created
    pub kafka_push_topic: String,    // 推送队列: flare.im.push.tasks
    pub kafka_timeout_ms: u64,
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

        Self {
            kafka_bootstrap,
            kafka_storage_topic,
            kafka_push_topic,
            kafka_timeout_ms,
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

