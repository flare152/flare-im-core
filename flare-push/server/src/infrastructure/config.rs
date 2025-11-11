use flare_im_core::config::{FlareAppConfig, RedisPoolConfig};
use std::env;

#[derive(Debug, Clone)]
pub struct PushServerConfig {
    pub kafka_bootstrap: String,
    pub consumer_group: String,
    pub message_topic: String,
    pub notification_topic: String,
    pub task_topic: String,
    pub kafka_timeout_ms: u64,
    pub redis_url: String,
    pub online_ttl_seconds: u64,
    pub default_tenant_id: String,
    pub hook_config: Option<String>,
    pub hook_config_dir: Option<String>,
}

impl PushServerConfig {
    pub fn from_app_config(app: &FlareAppConfig) -> Self {
        let service = app.push_server_service();
        let kafka_name = service.kafka.as_deref().unwrap_or("push");
        let redis_name = service.redis.as_deref().unwrap_or("session_store");

        let kafka_profile = app.kafka_profile(kafka_name);
        let redis_profile: Option<RedisPoolConfig> = app.redis_profile(redis_name).cloned();

        let kafka_bootstrap = env::var("PUSH_SERVER_KAFKA_BOOTSTRAP")
            .ok()
            .or_else(|| kafka_profile.map(|cfg| cfg.bootstrap_servers.clone()))
            .unwrap_or_else(|| "localhost:9092".to_string());

        let consumer_group =
            env::var("PUSH_SERVER_CONSUMER_GROUP").unwrap_or_else(|_| "push-server".to_string());

        let message_topic = env::var("PUSH_SERVER_MESSAGE_TOPIC")
            .ok()
            .or_else(|| service.message_topic.clone())
            .unwrap_or_else(|| "push-messages".to_string());

        let notification_topic = env::var("PUSH_SERVER_NOTIFICATION_TOPIC")
            .ok()
            .or_else(|| service.notification_topic.clone())
            .unwrap_or_else(|| "push-notifications".to_string());

        let task_topic = env::var("PUSH_SERVER_TASK_TOPIC")
            .ok()
            .or_else(|| service.task_topic.clone())
            .unwrap_or_else(|| "push-tasks".to_string());

        let redis_url = env::var("PUSH_SERVER_REDIS_URL")
            .ok()
            .or_else(|| redis_profile.as_ref().map(|cfg| cfg.url.clone()))
            .unwrap_or_else(|| "redis://127.0.0.1/".to_string());

        let kafka_timeout_ms = env::var("PUSH_SERVER_KAFKA_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(5_000);

        let online_ttl_seconds = env::var("PUSH_SERVER_ONLINE_TTL")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .or_else(|| service.online_ttl_seconds)
            .or_else(|| redis_profile.as_ref().and_then(|cfg| cfg.ttl_seconds))
            .unwrap_or(3_600);

        let default_tenant_id = env::var("PUSH_SERVER_DEFAULT_TENANT_ID")
            .ok()
            .or_else(|| service.default_tenant_id.clone())
            .unwrap_or_else(|| "default".to_string());

        let hook_config = env::var("PUSH_SERVER_HOOKS_CONFIG")
            .ok()
            .or_else(|| service.hook_config.clone());

        let hook_config_dir = env::var("PUSH_SERVER_HOOKS_CONFIG_DIR")
            .ok()
            .or_else(|| service.hook_config_dir.clone());

        Self {
            kafka_bootstrap,
            consumer_group,
            message_topic,
            notification_topic,
            task_topic,
            kafka_timeout_ms,
            redis_url,
            online_ttl_seconds,
            default_tenant_id,
            hook_config,
            hook_config_dir,
        }
    }
}
