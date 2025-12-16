//! 推送服务配置模块

use flare_im_core::config::{FlareAppConfig, RedisPoolConfig};
use flare_server_core::kafka::{KafkaConsumerConfig, KafkaProducerConfig};
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
    // 批量消费配置
    pub max_poll_records: usize,
    pub fetch_min_bytes: usize,
    pub fetch_max_wait_ms: u64,
    // 批量查询在线状态配置
    pub online_status_batch_size: usize,
    pub online_status_timeout_ms: u64,
    // Gateway Router配置
    pub gateway_router_connection_pool_size: usize,
    pub gateway_router_connection_timeout_ms: u64,
    pub gateway_router_connection_idle_timeout_ms: u64,
    pub gateway_deployment_mode: String, // "single_region" | "multi_region"
    pub local_gateway_id: Option<String>,
    // 注意：服务名已统一在 service_names.rs 中定义，不再在配置中存储
    // 所有服务名都直接从 service_names 模块获取，支持环境变量覆盖
    // 推送重试配置
    pub push_retry_max_attempts: u32,
    pub push_retry_initial_delay_ms: u64,
    pub push_retry_max_delay_ms: u64,
    pub push_retry_backoff_multiplier: f64,
    // ACK超时配置
    pub ack_timeout_seconds: u64,
    pub ack_monitor_interval_seconds: u64,
    pub ack_timeout_max_retries: u32,
    // ACK 监控性能优化配置
    pub ack_scan_batch_size: usize,          // 每次 SCAN 的 keys 数量
    pub ack_pipeline_batch_size: usize,      // Pipeline 批量获取的批次大小
    pub ack_timeout_batch_size: usize,       // 批量处理超时事件的批次大小
    pub ack_timeout_concurrent_limit: usize, // 并发处理超时事件的最大数量
    // ACK 服务配置（从业务模块配置中读取）
    pub ack_redis_ttl: u64,         // Redis 默认过期时间（秒）
    pub ack_cache_capacity: usize,  // 内存缓存容量
    pub ack_batch_interval_ms: u64, // 批量处理间隔（毫秒）
    pub ack_batch_size: usize,      // 批量处理大小
    // ACK 超时重试配置（区别于推送重试，避免 Kafka 阻塞）
    pub ack_retry_initial_delay_ms: u64, // ACK 超时重试初始延迟（毫秒，较短）
    pub ack_retry_max_delay_ms: u64,     // ACK 超时重试最大延迟（毫秒，较短）
    // 离线推送队列
    pub offline_topic: String,
    pub dlq_topic: String,
    // ACK Topic（从 Access Gateway 接收客户端 ACK）
    pub ack_topic: String,
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
            .unwrap_or_else(|| "127.0.0.1:29092".to_string());

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

        // 批量消费配置
        let max_poll_records = env::var("PUSH_SERVER_MAX_POLL_RECORDS")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(500);

        let fetch_min_bytes = env::var("PUSH_SERVER_FETCH_MIN_BYTES")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(1_048_576); // 1MB

        let fetch_max_wait_ms = env::var("PUSH_SERVER_FETCH_MAX_WAIT_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(100);

        // 批量查询在线状态配置
        let online_status_batch_size = env::var("PUSH_SERVER_ONLINE_STATUS_BATCH_SIZE")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(100);

        let online_status_timeout_ms = env::var("PUSH_SERVER_ONLINE_STATUS_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(50);

        // Gateway Router配置
        let gateway_router_connection_pool_size = env::var("PUSH_SERVER_GATEWAY_ROUTER_POOL_SIZE")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(10);

        let gateway_router_connection_timeout_ms =
            env::var("PUSH_SERVER_GATEWAY_ROUTER_TIMEOUT_MS")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(5000);

        let gateway_router_connection_idle_timeout_ms =
            env::var("PUSH_SERVER_GATEWAY_ROUTER_IDLE_TIMEOUT_MS")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(300_000); // 5分钟默认空闲超时

        let gateway_deployment_mode =
            env::var("GATEWAY_DEPLOYMENT_MODE").unwrap_or_else(|_| "single_region".to_string());

        let local_gateway_id = env::var("LOCAL_GATEWAY_ID").ok();

        // 注意：服务名已统一在 service_names.rs 中定义
        // 所有服务注册和发现都直接使用常量，不再从配置文件读取
        // 支持通过环境变量覆盖（例如：SESSION_SERVICE=flare-session-dev）

        // 推送重试配置
        let push_retry_max_attempts = env::var("PUSH_SERVER_RETRY_MAX_ATTEMPTS")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(3);

        let push_retry_initial_delay_ms = env::var("PUSH_SERVER_RETRY_INITIAL_DELAY_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(100);

        let push_retry_max_delay_ms = env::var("PUSH_SERVER_RETRY_MAX_DELAY_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(5000);

        let push_retry_backoff_multiplier = env::var("PUSH_SERVER_RETRY_BACKOFF_MULTIPLIER")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(2.0);

        // ACK超时配置
        let ack_timeout_seconds = env::var("PUSH_SERVER_ACK_TIMEOUT_SECONDS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(30);

        let ack_monitor_interval_seconds = env::var("PUSH_SERVER_ACK_MONITOR_INTERVAL_SECONDS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(1);

        let ack_timeout_max_retries = env::var("PUSH_SERVER_ACK_TIMEOUT_MAX_RETRIES")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(3);

        // 离线推送队列
        let offline_topic = env::var("PUSH_SERVER_OFFLINE_TOPIC")
            .unwrap_or_else(|_| "flare.im.push.offline".to_string());

        let dlq_topic =
            env::var("PUSH_SERVER_DLQ_TOPIC").unwrap_or_else(|_| "flare.im.push.dlq".to_string());

        let ack_topic =
            env::var("PUSH_SERVER_ACK_TOPIC").unwrap_or_else(|_| "flare.im.push.acks".to_string());

        // ACK 监控性能优化配置
        let ack_scan_batch_size = env::var("PUSH_SERVER_ACK_SCAN_BATCH_SIZE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(500); // 每次 SCAN 最多 500 个 keys

        let ack_pipeline_batch_size = env::var("PUSH_SERVER_ACK_PIPELINE_BATCH_SIZE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1000); // Pipeline 每批最多 1000 个 keys

        let ack_timeout_batch_size = env::var("PUSH_SERVER_ACK_TIMEOUT_BATCH_SIZE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(100); // 批量处理超时事件，每批 100 个

        let ack_timeout_concurrent_limit = env::var("PUSH_SERVER_ACK_TIMEOUT_CONCURRENT_LIMIT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(50); // 最多并发处理 50 个超时事件

        // ACK 服务配置（从业务模块配置中读取）
        let ack_redis_ttl = service
            .ack
            .as_ref()
            .map(|ack| ack.redis_ttl)
            .unwrap_or(3600);

        let ack_cache_capacity = service
            .ack
            .as_ref()
            .map(|ack| ack.cache_capacity)
            .unwrap_or(10000);

        let ack_batch_interval_ms = service
            .ack
            .as_ref()
            .map(|ack| ack.batch_interval_ms)
            .unwrap_or(100);

        let ack_batch_size = service
            .ack
            .as_ref()
            .map(|ack| ack.batch_size)
            .unwrap_or(100);

        // ACK 超时重试配置（区别于推送重试，避免 Kafka 阻塞）
        // ACK 超时重试应该更快，避免阻塞 Kafka 消费
        let ack_retry_initial_delay_ms = env::var("PUSH_SERVER_ACK_RETRY_INITIAL_DELAY_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(50); // 50ms，比推送重试更快

        let ack_retry_max_delay_ms = env::var("PUSH_SERVER_ACK_RETRY_MAX_DELAY_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1000); // 1秒，比推送重试更短，避免阻塞 Kafka

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
            max_poll_records,
            fetch_min_bytes,
            fetch_max_wait_ms,
            online_status_batch_size,
            online_status_timeout_ms,
            gateway_router_connection_pool_size,
            gateway_router_connection_timeout_ms,
            gateway_router_connection_idle_timeout_ms,
            gateway_deployment_mode,
            local_gateway_id,
            push_retry_max_attempts,
            push_retry_initial_delay_ms,
            push_retry_max_delay_ms,
            push_retry_backoff_multiplier,
            ack_timeout_seconds,
            ack_monitor_interval_seconds,
            ack_timeout_max_retries,
            ack_scan_batch_size,
            ack_pipeline_batch_size,
            ack_timeout_batch_size,
            ack_timeout_concurrent_limit,
            ack_redis_ttl,
            ack_cache_capacity,
            ack_batch_interval_ms,
            ack_batch_size,
            ack_retry_initial_delay_ms,
            ack_retry_max_delay_ms,
            offline_topic,
            dlq_topic,
            ack_topic,
        }
    }
}

// 实现 KafkaConsumerConfig trait，使 PushServerConfig 可以使用通用的 Kafka 消费者构建器
impl KafkaConsumerConfig for PushServerConfig {
    fn kafka_bootstrap(&self) -> &str {
        &self.kafka_bootstrap
    }

    fn consumer_group(&self) -> &str {
        &self.consumer_group
    }

    fn kafka_topic(&self) -> &str {
        &self.task_topic
    }

    fn fetch_min_bytes(&self) -> usize {
        self.fetch_min_bytes
    }

    fn fetch_max_wait_ms(&self) -> u64 {
        self.fetch_max_wait_ms
    }

    // 使用默认值，或根据需要覆盖
    fn session_timeout_ms(&self) -> u64 {
        30000
    }

    fn enable_auto_commit(&self) -> bool {
        false
    }

    fn auto_offset_reset(&self) -> &str {
        "earliest"
    }
}

// 实现 KafkaProducerConfig trait，使 PushServerConfig 可以使用通用的 Kafka 生产者构建器
impl KafkaProducerConfig for PushServerConfig {
    fn kafka_bootstrap(&self) -> &str {
        &self.kafka_bootstrap
    }

    fn message_timeout_ms(&self) -> u64 {
        self.kafka_timeout_ms
    }

    // 使用默认值，或根据需要覆盖
    fn enable_idempotence(&self) -> bool {
        true // 推送任务需要保证不丢失
    }

    fn compression_type(&self) -> &str {
        "snappy" // 推送任务使用 snappy 压缩，平衡性能和压缩比
    }
}
