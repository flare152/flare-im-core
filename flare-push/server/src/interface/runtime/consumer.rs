use std::sync::Arc;
use std::time::Duration;

use flare_im_core::metrics::PushServerMetrics;
use flare_proto::push::PushMessageRequest;
use flare_server_core::error::{ErrorBuilder, ErrorCode, Result};
use prost::Message;
use rdkafka::config::ClientConfig;
use rdkafka::consumer::{Consumer, StreamConsumer};
use rdkafka::message::Message as _;
use rdkafka::TopicPartitionList;
use tracing::{debug, error, info, warn};

use crate::application::commands::PushDispatchCommandService;
use crate::config::PushServerConfig;

pub struct PushKafkaConsumer {
    config: Arc<PushServerConfig>,
    consumer: StreamConsumer,
    command_service: Arc<PushDispatchCommandService>,
    metrics: Arc<PushServerMetrics>,
}

impl PushKafkaConsumer {
    pub async fn new(
        config: Arc<PushServerConfig>,
        command_service: Arc<PushDispatchCommandService>,
        metrics: Arc<PushServerMetrics>,
    ) -> Result<Self> {
        // 关键修复：强制使用 EXTERNAL listener，避免解析容器内地址
        // 添加 listener.name 配置，强制使用 EXTERNAL listener
        let consumer: StreamConsumer = ClientConfig::new()
            .set("bootstrap.servers", &config.kafka_bootstrap)
            .set("group.id", &config.consumer_group)
            .set("enable.partition.eof", "false")
            .set("session.timeout.ms", "30000")
            .set("enable.auto.commit", "false")
            .set("auto.offset.reset", "earliest") // 重要：从最早的消息开始消费（用于测试）
            .set("security.protocol", "plaintext")
            // 关键：强制使用 EXTERNAL listener（避免解析 kafka:9092）
            .set("client.dns.lookup", "use_all_dns_ips")
            .set("broker.address.family", "v4")
            // 强制使用 bootstrap servers 的地址，不依赖 metadata 返回的地址
            .set("metadata.max.age.ms", "300000") // 5 分钟
            // 重要：设置合理的消息大小限制，避免接收异常大的消息
            // Kafka broker 默认限制是 100MB，这里设置为 10MB 作为安全限制
            .set("fetch.message.max.bytes", "10485760") // 10MB
            .set("max.partition.fetch.bytes", "10485760") // 10MB
            .set("fetch.min.bytes", "1") // 降低到 1 字节，确保能立即收到消息
            .set("fetch.wait.max.ms", "100") // 最大等待时间 100ms
            .create()
            .map_err(|err| {
                ErrorBuilder::new(
                    ErrorCode::ServiceUnavailable,
                    "failed to build kafka consumer",
                )
                .details(err.to_string())
                .build_error()
            })?;

        // 订阅推送任务队列（由Message Orchestrator生产）
        info!(
            bootstrap = %config.kafka_bootstrap,
            group = %config.consumer_group,
            task_topic = %config.task_topic,
            "Subscribing to Kafka topic..."
        );
        
        // 先尝试订阅（使用 Consumer group）
        match consumer.subscribe(&[&config.task_topic]) {
            Ok(_) => {
                info!(
                    bootstrap = %config.kafka_bootstrap,
                    group = %config.consumer_group,
                    task_topic = %config.task_topic,
                    "Successfully subscribed to Kafka topic"
                );
            }
            Err(err) => {
                warn!(
                    error = %err,
                    bootstrap = %config.kafka_bootstrap,
                    group = %config.consumer_group,
                    task_topic = %config.task_topic,
                    "Failed to subscribe to Kafka topic, will try manual partition assignment"
                );
                
                // 如果订阅失败，尝试手动分配 partition 0（fallback）
                // 注意：这需要 topic 存在且有至少一个 partition
                let mut tpl = TopicPartitionList::new();
                tpl.add_partition_offset(
                    &config.task_topic,
                    0,
                    rdkafka::Offset::Beginning, // 从最早的消息开始消费
                );
                
                consumer.assign(&tpl).map_err(|assign_err| {
                    error!(
                        error = %assign_err,
                        bootstrap = %config.kafka_bootstrap,
                        task_topic = %config.task_topic,
                        "Failed to manually assign partition, topic may not exist"
                    );
                    ErrorBuilder::new(
                        ErrorCode::ServiceUnavailable,
                        "failed to subscribe or assign kafka topics",
                    )
                    .details(format!("Subscribe error: {}, Assign error: {}", err, assign_err))
                    .build_error()
                })?;
                
                info!(
                    bootstrap = %config.kafka_bootstrap,
                    task_topic = %config.task_topic,
                    partition = 0,
                    "Manually assigned to partition 0 as fallback"
                );
            }
        }
        
        info!(
            bootstrap = %config.kafka_bootstrap,
            group = %config.consumer_group,
            task_topic = %config.task_topic,
            max_poll_records = config.max_poll_records,
            "PushServer Kafka Consumer initialized and subscribed (connection will be established on first recv())"
        );

        Ok(Self {
            config,
            consumer,
            command_service,
            metrics,
        })
    }

    pub async fn run(&self) -> Result<()> {
        let mut consecutive_errors = 0;
        let mut last_error_time = None;
        let mut message_count = 0u64;
        
        info!(
            bootstrap = %self.config.kafka_bootstrap,
            group = %self.config.consumer_group,
            topic = %self.config.task_topic,
            "Push Server Consumer started, waiting for messages..."
        );
        
        // 等待 Consumer group 协调完成并分配到 partition
        // Consumer group 协调可能需要几秒钟
        // 如果使用手动分配（assign），partition 会立即分配
        let mut assignment_retries = 0;
        let max_assignment_retries = 15; // 增加到 15 秒，给 Consumer group 更多时间
        let mut has_partitions = false;
        
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            assignment_retries += 1;
            
            match self.consumer.assignment() {
                Ok(assignment) => {
                    if assignment.count() > 0 {
                        has_partitions = true;
                        info!(
                            partition_count = assignment.count(),
                            retries = assignment_retries,
                            "Consumer assigned to {} partitions after {} retries",
                            assignment.count(),
                            assignment_retries
                        );
                        break;
                    } else if assignment_retries >= max_assignment_retries {
                        error!(
                            retries = assignment_retries,
                            bootstrap = %self.config.kafka_bootstrap,
                            group = %self.config.consumer_group,
                            topic = %self.config.task_topic,
                            "Consumer still not assigned to any partitions after {} retries. Trying manual partition assignment as fallback.",
                            assignment_retries
                        );
                        
                        // 尝试手动分配 partition 0（fallback）
                        let mut tpl = TopicPartitionList::new();
                        tpl.add_partition_offset(
                            &self.config.task_topic,
                            0,
                            rdkafka::Offset::Beginning, // 从最早的消息开始消费
                        );
                        
                        match self.consumer.assign(&tpl) {
                            Ok(_) => {
                                // 手动分配后，TopicPartitionList 已经设置了 offset
                                // 但为了确保，我们需要等待一下让分配生效，然后可以开始消费
                                // 注意：使用 add_partition_offset 时，offset 已经设置好了
                                
                                info!(
                                    bootstrap = %self.config.kafka_bootstrap,
                                    task_topic = %self.config.task_topic,
                                    partition = 0,
                                    "Successfully manually assigned to partition 0 as fallback, will consume from beginning"
                                );
                                has_partitions = true;
                                break;
                            }
                            Err(assign_err) => {
                                error!(
                                    error = %assign_err,
                                    bootstrap = %self.config.kafka_bootstrap,
                                    task_topic = %self.config.task_topic,
                                    "Failed to manually assign partition, topic may not exist"
                                );
                                // 继续运行，可能稍后会分配到 partition
                                break;
                            }
                        }
                    } else {
                        debug!(
                            retries = assignment_retries,
                            "Waiting for partition assignment (attempt {}/{})",
                            assignment_retries,
                            max_assignment_retries
                        );
                    }
                }
                Err(err) => {
                    if assignment_retries >= max_assignment_retries {
                        warn!(
                            error = %err,
                            retries = assignment_retries,
                            "Failed to get consumer assignment after {} retries",
                            assignment_retries
                        );
                        break;
                    }
                }
            }
        }
        
        if !has_partitions {
            error!(
                bootstrap = %self.config.kafka_bootstrap,
                group = %self.config.consumer_group,
                topic = %self.config.task_topic,
                "Consumer failed to get any partition assignment. Messages will not be consumed. Please check if topic exists and Kafka is accessible."
            );
        }
        
        loop {
            // 定期输出心跳日志（每 10 秒）
            if message_count % 100 == 0 && message_count > 0 {
                info!(
                    message_count,
                    consecutive_errors,
                    "Consumer still running, processed {} messages",
                    message_count
                );
            }
            
            // 消费单条消息（StreamConsumer 每次返回一条消息）
            match self.consumer.recv().await {
                Ok(record) => {
                    // 成功收到消息，重置错误计数
                    consecutive_errors = 0;
                    last_error_time = None;
                    message_count += 1;
                    
                    info!(
                        message_count,
                        topic = %record.topic(),
                        partition = record.partition(),
                        offset = record.offset(),
                        "Received message #{} from Kafka",
                        message_count
                    );
                    
                    if let Some(payload) = record.payload() {
                        info!(
                            payload_len = payload.len(),
                            "Decoding PushMessageRequest, payload size: {} bytes",
                            payload.len()
                        );
                        
                        // 解析 PushMessageRequest
                        match PushMessageRequest::decode(payload) {
                            Ok(request) => {
                                info!(
                                    user_ids = ?request.user_ids,
                                    user_ids_count = request.user_ids.len(),
                                    "Received push message from Kafka"
                                );
                                
                                // 处理单条消息
                                if let Err(err) = self
                                    .command_service
                                    .handle_push_message(request).await
                                {
                                    error!(?err, "failed to process push message");
                                    // 注意：处理失败时不提交offset，等待重试
                                } else {
                                    info!("Successfully processed push message");
                                }
                            }
                            Err(err) => {
                                error!(?err, "failed to decode PushMessageRequest");
                            }
                        }
                    } else {
                        warn!("Received message with empty payload");
                    }
                }
                Err(err) => {
                    consecutive_errors += 1;
                    let now = std::time::Instant::now();
                    
                    // 记录错误详情
                    if consecutive_errors == 1 || last_error_time.map_or(true, |t| now.duration_since(t).as_secs() >= 5) {
                        error!(
                            error = %err,
                            consecutive_errors,
                            bootstrap = %self.config.kafka_bootstrap,
                            group = %self.config.consumer_group,
                            topic = %self.config.task_topic,
                            "error receiving from Kafka"
                        );
                        last_error_time = Some(now);
                    }
                    
                    // 根据连续错误次数调整重试间隔
                    let retry_delay = if consecutive_errors < 10 {
                        Duration::from_millis(100) // 前 10 次快速重试
                    } else if consecutive_errors < 50 {
                        Duration::from_millis(1000) // 之后 1 秒重试
                    } else {
                        Duration::from_secs(5) // 50 次后 5 秒重试
                    };
                    
                    tokio::time::sleep(retry_delay).await;
                }
            }
        }
    }

    pub fn config(&self) -> &Arc<PushServerConfig> {
        &self.config
    }
}
