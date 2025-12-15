use flare_im_core::config::FlareAppConfig;
use flare_server_core::kafka::KafkaProducerConfig;

#[derive(Debug, Clone)]
pub struct PushProxyConfig {
    pub kafka_bootstrap: String,
    pub message_topic: String,
    pub notification_topic: String,
    pub ack_topic: String,  // ACK Topic（从 Gateway 接收客户端 ACK）
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
            ack_topic: std::env::var("PUSH_PROXY_ACK_TOPIC")
                .or_else(|_| service.ack_topic.clone().ok_or(std::env::VarError::NotPresent))
                .unwrap_or_else(|_| "flare.im.push.acks".to_string()),
            kafka_timeout_ms: service
                .timeout_ms
                .or_else(|| kafka_profile.and_then(|cfg| cfg.timeout_ms))
                .unwrap_or(5_000),
        }
    }
}

// 实现 KafkaProducerConfig trait，使 PushProxyConfig 可以使用通用的 Kafka 生产者构建器
impl KafkaProducerConfig for PushProxyConfig {
    fn kafka_bootstrap(&self) -> &str {
        &self.kafka_bootstrap
    }
    
    fn message_timeout_ms(&self) -> u64 {
        self.kafka_timeout_ms
    }
    
    // 使用默认值，或根据需要覆盖
    fn enable_idempotence(&self) -> bool {
        true // 推送代理需要保证消息不丢失
    }
    
    fn compression_type(&self) -> &str {
        "snappy" // 使用 snappy 压缩
    }
}
