//! # Prometheus 指标收集模块
//!
//! 为各个服务模块提供统一的 Prometheus 指标收集能力。

use once_cell::sync::Lazy;
use prometheus::{
    Histogram, HistogramOpts, HistogramVec, IntCounter, IntCounterVec, IntGauge,
    Opts, Registry,
};

/// 全局指标注册表
pub static REGISTRY: Lazy<Registry> = Lazy::new(Registry::new);

/// 消息编排服务指标
pub struct MessageOrchestratorMetrics {
    /// 消息发送总数
    pub messages_sent_total: IntCounterVec,
    /// 消息发送耗时（秒）
    pub messages_sent_duration_seconds: Histogram,
    /// PreSend Hook 耗时（秒）
    pub pre_send_hook_duration_seconds: Histogram,
    /// WAL 写入耗时（秒）
    pub wal_write_duration_seconds: Histogram,
    /// Kafka 生产耗时（秒）
    pub kafka_produce_duration_seconds: Histogram,
    /// PreSend Hook 失败次数
    pub pre_send_hook_failure_total: IntCounterVec,
    /// WAL 写入失败次数
    pub wal_write_failure_total: IntCounter,
    /// Kafka 生产失败次数
    pub kafka_produce_failure_total: IntCounterVec,
}

impl MessageOrchestratorMetrics {
    pub fn new() -> Self {
        let messages_sent_total = IntCounterVec::new(
            Opts::new(
                "messages_sent_total",
                "Total number of messages sent",
            ),
            &["message_type", "tenant_id"],
        )
        .expect("Failed to create messages_sent_total metric");

        let messages_sent_duration_seconds = Histogram::with_opts(
            HistogramOpts::new(
                "messages_sent_duration_seconds",
                "Message sending duration in seconds",
            )
            .buckets(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0]),
        )
        .expect("Failed to create messages_sent_duration_seconds metric");

        let pre_send_hook_duration_seconds = Histogram::with_opts(
            HistogramOpts::new(
                "pre_send_hook_duration_seconds",
                "PreSend Hook execution duration in seconds",
            )
            .buckets(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0]),
        )
        .expect("Failed to create pre_send_hook_duration_seconds metric");

        let wal_write_duration_seconds = Histogram::with_opts(
            HistogramOpts::new(
                "wal_write_duration_seconds",
                "WAL write duration in seconds",
            )
            .buckets(vec![0.001, 0.005, 0.01, 0.05, 0.1]),
        )
        .expect("Failed to create wal_write_duration_seconds metric");

        let kafka_produce_duration_seconds = Histogram::with_opts(
            HistogramOpts::new(
                "kafka_produce_duration_seconds",
                "Kafka produce duration in seconds",
            )
            .buckets(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5]),
        )
        .expect("Failed to create kafka_produce_duration_seconds metric");

        let pre_send_hook_failure_total = IntCounterVec::new(
            Opts::new(
                "pre_send_hook_failure_total",
                "Total number of PreSend Hook failures",
            ),
            &["hook_name", "tenant_id"],
        )
        .expect("Failed to create pre_send_hook_failure_total metric");

        let wal_write_failure_total = IntCounter::new(
            "wal_write_failure_total",
            "Total number of WAL write failures",
        )
        .expect("Failed to create wal_write_failure_total metric");

        let kafka_produce_failure_total = IntCounterVec::new(
            Opts::new(
                "kafka_produce_failure_total",
                "Total number of Kafka produce failures",
            ),
            &["topic", "tenant_id"],
        )
        .expect("Failed to create kafka_produce_failure_total metric");

        // 注册指标，忽略重复注册错误（在基准测试中可能会重复创建）
        let _ = REGISTRY.register(Box::new(messages_sent_total.clone()));
        let _ = REGISTRY.register(Box::new(messages_sent_duration_seconds.clone()));
        let _ = REGISTRY.register(Box::new(pre_send_hook_duration_seconds.clone()));
        let _ = REGISTRY.register(Box::new(wal_write_duration_seconds.clone()));
        let _ = REGISTRY.register(Box::new(kafka_produce_duration_seconds.clone()));
        let _ = REGISTRY.register(Box::new(pre_send_hook_failure_total.clone()));
        let _ = REGISTRY.register(Box::new(wal_write_failure_total.clone()));
        let _ = REGISTRY.register(Box::new(kafka_produce_failure_total.clone()));

        Self {
            messages_sent_total,
            messages_sent_duration_seconds,
            pre_send_hook_duration_seconds,
            wal_write_duration_seconds,
            kafka_produce_duration_seconds,
            pre_send_hook_failure_total,
            wal_write_failure_total,
            kafka_produce_failure_total,
        }
    }
}

impl Default for MessageOrchestratorMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// 存储写入服务指标
pub struct StorageWriterMetrics {
    /// 消息持久化总数
    pub messages_persisted_total: IntCounterVec,
    /// 消息持久化耗时（秒）
    pub messages_persisted_duration_seconds: Histogram,
    /// 数据库写入耗时（秒）
    pub db_write_duration_seconds: Histogram,
    /// Redis 更新耗时（秒）
    pub redis_update_duration_seconds: Histogram,
    /// 消息重复处理次数
    pub messages_duplicate_total: IntCounter,
    /// 批量处理大小
    pub batch_size: Histogram,
}

impl StorageWriterMetrics {
    pub fn new() -> Self {
        let messages_persisted_total = IntCounterVec::new(
            Opts::new(
                "messages_persisted_total",
                "Total number of messages persisted",
            ),
            &["tenant_id"],
        )
        .expect("Failed to create messages_persisted_total metric");

        let messages_persisted_duration_seconds = Histogram::with_opts(
            HistogramOpts::new(
                "messages_persisted_duration_seconds",
                "Message persistence duration in seconds",
            )
            .buckets(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0]),
        )
        .expect("Failed to create messages_persisted_duration_seconds metric");

        let db_write_duration_seconds = Histogram::with_opts(
            HistogramOpts::new(
                "db_write_duration_seconds",
                "Database write duration in seconds",
            )
            .buckets(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5]),
        )
        .expect("Failed to create db_write_duration_seconds metric");

        let redis_update_duration_seconds = Histogram::with_opts(
            HistogramOpts::new(
                "redis_update_duration_seconds",
                "Redis update duration in seconds",
            )
            .buckets(vec![0.0001, 0.0005, 0.001, 0.005, 0.01]),
        )
        .expect("Failed to create redis_update_duration_seconds metric");

        let messages_duplicate_total = IntCounter::new(
            "messages_duplicate_total",
            "Total number of duplicate messages",
        )
        .expect("Failed to create messages_duplicate_total metric");

        let batch_size = Histogram::with_opts(
            HistogramOpts::new(
                "storage_writer_batch_size",
                "Batch size for storage writer",
            )
            .buckets(vec![1.0, 10.0, 50.0, 100.0, 500.0, 1000.0]),
        )
        .expect("Failed to create batch_size metric");

        // 注册指标，忽略重复注册错误（在基准测试中可能会重复创建）
        let _ = REGISTRY.register(Box::new(messages_persisted_total.clone()));
        let _ = REGISTRY.register(Box::new(messages_persisted_duration_seconds.clone()));
        let _ = REGISTRY.register(Box::new(db_write_duration_seconds.clone()));
        let _ = REGISTRY.register(Box::new(redis_update_duration_seconds.clone()));
        let _ = REGISTRY.register(Box::new(messages_duplicate_total.clone()));
        let _ = REGISTRY.register(Box::new(batch_size.clone()));

        Self {
            messages_persisted_total,
            messages_persisted_duration_seconds,
            db_write_duration_seconds,
            redis_update_duration_seconds,
            messages_duplicate_total,
            batch_size,
        }
    }
}

impl Default for StorageWriterMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// 推送服务指标
pub struct PushServerMetrics {
    /// 推送任务处理总数
    pub push_tasks_processed_total: IntCounterVec,
    /// 在线推送成功次数
    pub online_push_success_total: IntCounterVec,
    /// 在线推送失败次数
    pub online_push_failure_total: IntCounterVec,
    /// 离线推送任务创建次数
    pub offline_push_tasks_created_total: IntCounterVec,
    /// 在线状态查询耗时（秒）
    pub online_status_query_duration_seconds: Histogram,
    /// 推送延迟（秒）
    pub push_latency_seconds: HistogramVec,
    /// 批量处理大小
    pub batch_size: Histogram,
    /// ACK确认次数
    pub ack_received_total: IntCounterVec,
    /// ACK超时次数
    pub ack_timeout_total: IntCounterVec,
}

impl PushServerMetrics {
    pub fn new() -> Self {
        let push_tasks_processed_total = IntCounterVec::new(
            Opts::new(
                "push_tasks_processed_total",
                "Total number of push tasks processed",
            ),
            &["message_type", "tenant_id"],
        )
        .expect("Failed to create push_tasks_processed_total metric");

        let online_push_success_total = IntCounterVec::new(
            Opts::new(
                "online_push_success_total",
                "Total number of successful online pushes",
            ),
            &["gateway_id", "tenant_id"],
        )
        .expect("Failed to create online_push_success_total metric");

        let online_push_failure_total = IntCounterVec::new(
            Opts::new(
                "online_push_failure_total",
                "Total number of failed online pushes",
            ),
            &["gateway_id", "failure_reason", "tenant_id"],
        )
        .expect("Failed to create online_push_failure_total metric");

        let offline_push_tasks_created_total = IntCounterVec::new(
            Opts::new(
                "offline_push_tasks_created_total",
                "Total number of offline push tasks created",
            ),
            &["message_type", "tenant_id"],
        )
        .expect("Failed to create offline_push_tasks_created_total metric");

        let online_status_query_duration_seconds = Histogram::with_opts(
            HistogramOpts::new(
                "online_status_query_duration_seconds",
                "Online status query duration in seconds",
            )
            .buckets(vec![0.001, 0.005, 0.01, 0.05, 0.1]),
        )
        .expect("Failed to create online_status_query_duration_seconds metric");

        let push_latency_seconds = HistogramVec::new(
            HistogramOpts::new(
                "push_latency_seconds",
                "Push latency in seconds",
            )
            .buckets(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0]),
            &["push_type", "tenant_id"],
        )
        .expect("Failed to create push_latency_seconds metric");

        let batch_size = Histogram::with_opts(
            HistogramOpts::new(
                "push_server_batch_size",
                "Batch size for push server",
            )
            .buckets(vec![1.0, 10.0, 50.0, 100.0, 500.0, 1000.0]),
        )
        .expect("Failed to create batch_size metric");

        let ack_received_total = IntCounterVec::new(
            Opts::new(
                "ack_received_total",
                "Total number of ACKs received",
            ),
            &["ack_type", "tenant_id"],
        )
        .expect("Failed to create ack_received_total metric");

        let ack_timeout_total = IntCounterVec::new(
            Opts::new(
                "ack_timeout_total",
                "Total number of ACK timeouts",
            ),
            &["ack_type", "tenant_id"],
        )
        .expect("Failed to create ack_timeout_total metric");

        // 注册指标，忽略重复注册错误（在基准测试中可能会重复创建）
        let _ = REGISTRY.register(Box::new(push_tasks_processed_total.clone()));
        let _ = REGISTRY.register(Box::new(online_push_success_total.clone()));
        let _ = REGISTRY.register(Box::new(online_push_failure_total.clone()));
        let _ = REGISTRY.register(Box::new(offline_push_tasks_created_total.clone()));
        let _ = REGISTRY.register(Box::new(online_status_query_duration_seconds.clone()));
        let _ = REGISTRY.register(Box::new(push_latency_seconds.clone()));
        let _ = REGISTRY.register(Box::new(batch_size.clone()));
        let _ = REGISTRY.register(Box::new(ack_received_total.clone()));
        let _ = REGISTRY.register(Box::new(ack_timeout_total.clone()));

        Self {
            push_tasks_processed_total,
            online_push_success_total,
            online_push_failure_total,
            offline_push_tasks_created_total,
            online_status_query_duration_seconds,
            push_latency_seconds,
            batch_size,
            ack_received_total,
            ack_timeout_total,
        }
    }
}

impl Default for PushServerMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// 推送执行器（Worker）指标
pub struct PushWorkerMetrics {
    /// 离线推送成功次数
    pub offline_push_success_total: IntCounterVec,
    /// 离线推送失败次数
    pub offline_push_failure_total: IntCounterVec,
    /// 推送重试次数
    pub push_retry_total: IntCounterVec,
    /// 推送耗时（秒）
    pub push_duration_seconds: HistogramVec,
    /// 死信队列消息数
    pub dlq_messages_total: IntCounterVec,
    /// 批量处理大小
    pub batch_size: Histogram,
}

impl PushWorkerMetrics {
    pub fn new() -> Self {
        let offline_push_success_total = IntCounterVec::new(
            Opts::new(
                "offline_push_success_total",
                "Total number of successful offline pushes",
            ),
            &["platform", "tenant_id"],
        )
        .expect("Failed to create offline_push_success_total metric");

        let offline_push_failure_total = IntCounterVec::new(
            Opts::new(
                "offline_push_failure_total",
                "Total number of failed offline pushes",
            ),
            &["platform", "failure_reason", "tenant_id"],
        )
        .expect("Failed to create offline_push_failure_total metric");

        let push_retry_total = IntCounterVec::new(
            Opts::new(
                "push_retry_total",
                "Total number of push retries",
            ),
            &["platform", "retry_reason", "tenant_id"],
        )
        .expect("Failed to create push_retry_total metric");

        let push_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "push_duration_seconds",
                "Push duration in seconds",
            )
            .buckets(vec![0.01, 0.05, 0.1, 0.5, 1.0, 5.0]),
            &["platform", "tenant_id"],
        )
        .expect("Failed to create push_duration_seconds metric");

        let dlq_messages_total = IntCounterVec::new(
            Opts::new(
                "dlq_messages_total",
                "Total number of messages sent to dead letter queue",
            ),
            &["failure_reason", "tenant_id"],
        )
        .expect("Failed to create dlq_messages_total metric");

        let batch_size = Histogram::with_opts(
            HistogramOpts::new(
                "push_worker_batch_size",
                "Batch size for push worker",
            )
            .buckets(vec![1.0, 10.0, 50.0, 100.0, 500.0]),
        )
        .expect("Failed to create batch_size metric");

        // 注册指标，忽略重复注册错误（在基准测试中可能会重复创建）
        let _ = REGISTRY.register(Box::new(offline_push_success_total.clone()));
        let _ = REGISTRY.register(Box::new(offline_push_failure_total.clone()));
        let _ = REGISTRY.register(Box::new(push_retry_total.clone()));
        let _ = REGISTRY.register(Box::new(push_duration_seconds.clone()));
        let _ = REGISTRY.register(Box::new(dlq_messages_total.clone()));
        let _ = REGISTRY.register(Box::new(batch_size.clone()));

        Self {
            offline_push_success_total,
            offline_push_failure_total,
            push_retry_total,
            push_duration_seconds,
            dlq_messages_total,
            batch_size,
        }
    }
}

impl Default for PushWorkerMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Access Gateway 指标
pub struct AccessGatewayMetrics {
    /// 活跃连接数
    pub connections_active: IntGauge,
    /// 消息推送总数
    pub messages_pushed_total: IntCounterVec,
    /// 推送成功次数
    pub push_success_total: IntCounterVec,
    /// 推送失败次数
    pub push_failure_total: IntCounterVec,
    /// 连接断开次数
    pub connection_disconnected_total: IntCounter,
    /// 客户端ACK接收次数
    pub client_ack_received_total: IntCounterVec,
    /// 推送延迟（秒）
    pub push_latency_seconds: HistogramVec,
    /// 在线状态缓存命中率
    pub online_cache_hit_total: IntCounter,
    pub online_cache_miss_total: IntCounter,
}

impl AccessGatewayMetrics {
    pub fn new() -> Self {
        let connections_active = IntGauge::new(
            "connections_active",
            "Number of active connections",
        )
        .expect("Failed to create connections_active metric");

        let messages_pushed_total = IntCounterVec::new(
            Opts::new(
                "messages_pushed_total",
                "Total number of messages pushed",
            ),
            &["tenant_id"],
        )
        .expect("Failed to create messages_pushed_total metric");

        let push_success_total = IntCounterVec::new(
            Opts::new(
                "push_success_total",
                "Total number of successful pushes",
            ),
            &["tenant_id"],
        )
        .expect("Failed to create push_success_total metric");

        let push_failure_total = IntCounterVec::new(
            Opts::new(
                "push_failure_total",
                "Total number of failed pushes",
            ),
            &["failure_reason", "tenant_id"],
        )
        .expect("Failed to create push_failure_total metric");

        let connection_disconnected_total = IntCounter::new(
            "connection_disconnected_total",
            "Total number of disconnected connections",
        )
        .expect("Failed to create connection_disconnected_total metric");

        let client_ack_received_total = IntCounterVec::new(
            Opts::new(
                "client_ack_received_total",
                "Total number of client ACKs received",
            ),
            &["tenant_id"],
        )
        .expect("Failed to create client_ack_received_total metric");

        let push_latency_seconds = HistogramVec::new(
            HistogramOpts::new(
                "access_gateway_push_latency_seconds",
                "Push latency in seconds",
            )
            .buckets(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5]),
            &["tenant_id"],
        )
        .expect("Failed to create push_latency_seconds metric");

        let online_cache_hit_total = IntCounter::new(
            "online_cache_hit_total",
            "Total number of online cache hits",
        )
        .expect("Failed to create online_cache_hit_total metric");

        let online_cache_miss_total = IntCounter::new(
            "online_cache_miss_total",
            "Total number of online cache misses",
        )
        .expect("Failed to create online_cache_miss_total metric");

        REGISTRY
            .register(Box::new(connections_active.clone()))
            .unwrap();
        REGISTRY
            .register(Box::new(messages_pushed_total.clone()))
            .unwrap();
        REGISTRY
            .register(Box::new(push_success_total.clone()))
            .unwrap();
        REGISTRY
            .register(Box::new(push_failure_total.clone()))
            .unwrap();
        REGISTRY
            .register(Box::new(connection_disconnected_total.clone()))
            .unwrap();
        REGISTRY
            .register(Box::new(client_ack_received_total.clone()))
            .unwrap();
        REGISTRY
            .register(Box::new(push_latency_seconds.clone()))
            .unwrap();
        REGISTRY
            .register(Box::new(online_cache_hit_total.clone()))
            .unwrap();
        REGISTRY
            .register(Box::new(online_cache_miss_total.clone()))
            .unwrap();

        Self {
            connections_active,
            messages_pushed_total,
            push_success_total,
            push_failure_total,
            connection_disconnected_total,
            client_ack_received_total,
            push_latency_seconds,
            online_cache_hit_total,
            online_cache_miss_total,
        }
    }
}

impl Default for AccessGatewayMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// 获取 Prometheus 指标导出格式
pub fn gather_metrics() -> String {
    use prometheus::Encoder;
    let encoder = prometheus::TextEncoder::new();
    let metric_families = REGISTRY.gather();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer).unwrap();
    String::from_utf8(buffer).unwrap()
}

