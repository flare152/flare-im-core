//! 推送 Worker 配置

use flare_im_core::config::FlareAppConfig;
use flare_server_core::kafka::KafkaProducerConfig;
use std::env;

#[derive(Debug, Clone)]
pub struct PushWorkerConfig {
    pub kafka_bootstrap: String,
    pub consumer_group: String,
    pub task_topic: String, // 离线推送任务topic: flare.im.push.offline
    pub signaling_service: Option<String>, // 改为服务名
    pub offline_provider: Option<String>,
    pub hook_config: Option<String>,
    pub hook_config_dir: Option<String>,
    // 批量消费配置
    pub max_poll_records: usize,
    pub fetch_min_bytes: usize,
    pub fetch_max_wait_ms: u64,
    // 推送重试配置
    pub push_retry_max_attempts: u32,
    pub push_retry_initial_delay_ms: u64,
    pub push_retry_max_delay_ms: u64,
    pub push_retry_backoff_multiplier: f64,
    // ACK上报配置
    pub ack_topic: Option<String>, // ACK上报topic（可选，如果使用gRPC则不需要）
    pub ack_timeout_seconds: u64,
    // 死信队列配置
    pub dlq_topic: String, // 死信队列topic: flare.im.push.dlq
    // 推送渠道配置
    pub push_provider: String, // "fcm" | "apns" | "webpush" | "noop"
    // Gateway Router 配置
    pub access_gateway_service: Option<String>, // Access Gateway 服务名
}

impl PushWorkerConfig {
    pub fn from_app_config(app: &FlareAppConfig) -> Self {
        let service = app.push_worker_service();
        let kafka_name = service.kafka.as_deref().unwrap_or("push");
        let kafka_profile = app.kafka_profile(kafka_name);

        let kafka_bootstrap = env::var("PUSH_WORKER_KAFKA_BOOTSTRAP")
            .ok()
            .or_else(|| kafka_profile.map(|cfg| cfg.bootstrap_servers.clone()))
            .unwrap_or_else(|| "localhost:9092".to_string());

        let consumer_group =
            env::var("PUSH_WORKER_CONSUMER_GROUP").unwrap_or_else(|_| "push-worker".to_string());

        let task_topic = env::var("PUSH_WORKER_TASK_TOPIC")
            .ok()
            .or_else(|| service.task_topic.clone())
            .unwrap_or_else(|| "flare.im.push.offline".to_string());
        
        // 批量消费配置
        let max_poll_records = env::var("PUSH_WORKER_MAX_POLL_RECORDS")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(100);
        
        let fetch_min_bytes = env::var("PUSH_WORKER_FETCH_MIN_BYTES")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(1024);
        
        let fetch_max_wait_ms = env::var("PUSH_WORKER_FETCH_MAX_WAIT_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(100);
        
        // 推送重试配置
        let push_retry_max_attempts = env::var("PUSH_WORKER_RETRY_MAX_ATTEMPTS")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(3);
        
        let push_retry_initial_delay_ms = env::var("PUSH_WORKER_RETRY_INITIAL_DELAY_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(1000);
        
        let push_retry_max_delay_ms = env::var("PUSH_WORKER_RETRY_MAX_DELAY_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(30000);
        
        let push_retry_backoff_multiplier = env::var("PUSH_WORKER_RETRY_BACKOFF_MULTIPLIER")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(2.0);
        
        // ACK上报配置
        let ack_topic = env::var("PUSH_WORKER_ACK_TOPIC")
            .ok();
        
        let ack_timeout_seconds = env::var("PUSH_WORKER_ACK_TIMEOUT_SECONDS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(30);
        
        // 死信队列配置
        let dlq_topic = env::var("PUSH_WORKER_DLQ_TOPIC")
            .ok()
            .unwrap_or_else(|| "flare.im.push.dlq".to_string());
        
        // 推送渠道配置
        let push_provider = env::var("PUSH_WORKER_PUSH_PROVIDER")
            .ok()
            .unwrap_or_else(|| "noop".to_string());

        let signaling_service = env::var("PUSH_WORKER_SIGNALING_SERVICE")
            .ok();
        let offline_provider = env::var("PUSH_WORKER_OFFLINE_PROVIDER")
            .ok()
            .or_else(|| service.offline_provider.clone());

        let hook_config = env::var("PUSH_WORKER_HOOKS_CONFIG")
            .ok()
            .or_else(|| service.hook_config.clone());
        let hook_config_dir = env::var("PUSH_WORKER_HOOKS_CONFIG_DIR")
            .ok()
            .or_else(|| service.hook_config_dir.clone());

        let access_gateway_service = env::var("PUSH_WORKER_ACCESS_GATEWAY_SERVICE")
            .ok();

        Self {
            kafka_bootstrap,
            consumer_group,
            task_topic,
            signaling_service,
            offline_provider,
            hook_config,
            hook_config_dir,
            max_poll_records,
            fetch_min_bytes,
            fetch_max_wait_ms,
            push_retry_max_attempts,
            push_retry_initial_delay_ms,
            push_retry_max_delay_ms,
            push_retry_backoff_multiplier,
            ack_topic,
            ack_timeout_seconds,
            dlq_topic,
            push_provider,
            access_gateway_service,
        }
    }
}

// 实现 KafkaProducerConfig trait，使 PushWorkerConfig 可以使用通用的 Kafka 生产者构建器
impl KafkaProducerConfig for PushWorkerConfig {
    fn kafka_bootstrap(&self) -> &str {
        &self.kafka_bootstrap
    }
    
    fn message_timeout_ms(&self) -> u64 {
        5000 // 使用默认值 5 秒
    }
    
    // 使用默认值，或根据需要覆盖
    fn enable_idempotence(&self) -> bool {
        true // ACK 和 DLQ 消息需要保证不丢失
    }
    
    fn compression_type(&self) -> &str {
        "snappy" // 使用 snappy 压缩
    }
}

