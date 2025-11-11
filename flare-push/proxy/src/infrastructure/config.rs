use flare_im_core::config::FlareAppConfig;

#[derive(Debug, Clone)]
pub struct PushProxyConfig {
    pub kafka_bootstrap: String,
    pub message_topic: String,
    pub notification_topic: String,
    pub kafka_timeout_ms: u64,
}

impl PushProxyConfig {
    pub fn from_app_config(app: &FlareAppConfig) -> Self {
        let service = app.push_proxy_service();
        let kafka_profile = service
            .kafka
            .as_deref()
            .and_then(|name| app.kafka_profile(name));

        Self {
            kafka_bootstrap: kafka_profile
                .map(|cfg| cfg.bootstrap_servers.clone())
                .unwrap_or_else(|| "localhost:9092".to_string()),
            message_topic: service
                .message_topic
                .unwrap_or_else(|| "push-messages".to_string()),
            notification_topic: service
                .notification_topic
                .unwrap_or_else(|| "push-notifications".to_string()),
            kafka_timeout_ms: service
                .timeout_ms
                .or_else(|| kafka_profile.and_then(|cfg| cfg.timeout_ms))
                .unwrap_or(5_000),
        }
    }
}
