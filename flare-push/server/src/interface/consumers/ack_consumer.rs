//! ACK Kafka 消费者
//!
//! 消费 ACK Topic，处理客户端 ACK，停止重试

use std::sync::Arc;
use std::time::Duration;

use flare_proto::flare::push::v1::PushAckRequest;
use flare_server_core::error::{ErrorBuilder, ErrorCode, Result};
use prost::Message;
use rdkafka::consumer::{CommitMode, Consumer, StreamConsumer};
use rdkafka::message::{BorrowedMessage, Message as _};
use tracing::{debug, error, info, warn};

use crate::config::PushServerConfig;
use crate::domain::service::PushDomainService;
use flare_server_core::kafka::{
    KafkaConsumerConfig, build_kafka_consumer, subscribe_and_wait_for_assignment,
};

/// ACK Topic 消费者配置包装器
struct AckConsumerConfig {
    config: Arc<PushServerConfig>,
    consumer_group: String, // 存储独立的 consumer group 名称
}

impl KafkaConsumerConfig for AckConsumerConfig {
    fn kafka_bootstrap(&self) -> &str {
        &self.config.kafka_bootstrap
    }

    fn consumer_group(&self) -> &str {
        // 使用独立的 consumer group，避免与 push-tasks 消费者冲突
        &self.consumer_group
    }

    fn kafka_topic(&self) -> &str {
        &self.config.ack_topic
    }

    fn fetch_min_bytes(&self) -> usize {
        self.config.fetch_min_bytes
    }

    fn fetch_max_wait_ms(&self) -> u64 {
        self.config.fetch_max_wait_ms
    }

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

pub struct AckKafkaConsumer {
    config: Arc<PushServerConfig>,
    consumer: StreamConsumer,
    domain_service: Arc<PushDomainService>,
}

impl AckKafkaConsumer {
    pub async fn new(
        config: Arc<PushServerConfig>,
        domain_service: Arc<PushDomainService>,
    ) -> Result<Self> {
        // 创建 ACK 消费者配置包装器（使用独立的 consumer group）
        let consumer_group = format!("{}-ack", config.consumer_group);
        let ack_config = AckConsumerConfig {
            config: config.clone(),
            consumer_group: consumer_group.clone(),
        };

        // 使用统一的消费者构建器
        let consumer =
            build_kafka_consumer(&ack_config as &dyn KafkaConsumerConfig).map_err(|err| {
                ErrorBuilder::new(
                    ErrorCode::ServiceUnavailable,
                    "failed to build ACK kafka consumer",
                )
                .details(err.to_string())
                .build_error()
            })?;

        info!(
            bootstrap = %config.kafka_bootstrap,
            group = %ack_config.consumer_group(),
            ack_topic = %config.ack_topic,
            "Subscribing to ACK Kafka topic..."
        );

        // 订阅并等待 partition assignment（最多等待 15 秒）
        subscribe_and_wait_for_assignment(&consumer, &config.ack_topic, 15)
            .await
            .map_err(|err| {
                ErrorBuilder::new(
                    ErrorCode::ServiceUnavailable,
                    "failed to subscribe or assign ACK kafka topics",
                )
                .details(err)
                .build_error()
            })?;

        info!(
            bootstrap = %config.kafka_bootstrap,
            group = %ack_config.consumer_group(),
            ack_topic = %config.ack_topic,
            "ACK Kafka Consumer initialized and ready"
        );

        Ok(Self {
            config,
            consumer,
            domain_service,
        })
    }

    pub async fn run(&self) -> Result<()> {
        let mut consecutive_errors = 0;
        let mut last_error_time = None;
        let mut message_count = 0u64;

        let consumer_group = format!("{}-ack", self.config.consumer_group);
        info!(
            bootstrap = %self.config.kafka_bootstrap,
            group = %consumer_group,
            topic = %self.config.ack_topic,
            "ACK Consumer started, waiting for messages..."
        );

        loop {
            // 定期输出心跳日志
            if message_count % 100 == 0 && message_count > 0 {
                info!(
                    message_count,
                    consecutive_errors,
                    "ACK Consumer still running, processed {} ACK messages",
                    message_count
                );
            }

            // 消费单条消息
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
                        "Received ACK message #{} from Kafka",
                        message_count
                    );

                    if let Some(payload) = record.payload() {
                        info!(
                            payload_len = payload.len(),
                            "Decoding PushAckRequest, payload size: {} bytes",
                            payload.len()
                        );

                        // 解析 PushAckRequest
                        match PushAckRequest::decode(payload) {
                            Ok(request) => {
                                // 处理 ACK
                                if let Some(ack) = &request.ack {
                                    info!(
                                        message_id = %ack.server_msg_id,
                                        user_count = request.target_user_ids.len(),
                                        "Processing ACK from Kafka"
                                    );

                                    // 为每个用户处理 ACK
                                    for user_id in &request.target_user_ids {
                                        match self
                                            .domain_service
                                            .handle_client_ack(user_id, ack)
                                            .await
                                        {
                                            Ok(_) => {
                                                debug!(
                                                    message_id = %ack.server_msg_id,
                                                    user_id = %user_id,
                                                    "ACK processed successfully"
                                                );
                                            }
                                            Err(e) => {
                                                error!(
                                                    error = %e,
                                                    message_id = %ack.server_msg_id,
                                                    user_id = %user_id,
                                                    "Failed to process ACK"
                                                );
                                            }
                                        }
                                    }
                                } else {
                                    warn!("Received PushAckRequest with no ACK payload");
                                }

                                // 处理成功，提交 offset
                                self.commit_message(&record);
                            }
                            Err(e) => {
                                error!(
                                    error = %e,
                                    topic = %record.topic(),
                                    partition = record.partition(),
                                    offset = record.offset(),
                                    "Failed to decode PushAckRequest"
                                );
                                consecutive_errors += 1;

                                // 解码失败时也提交 offset，避免无限重试
                                warn!(
                                    "Decoding failed, committing offset to avoid blocking consumer"
                                );
                                self.commit_message(&record);
                            }
                        }
                    } else {
                        warn!(
                            topic = %record.topic(),
                            partition = record.partition(),
                            offset = record.offset(),
                            "Received message with empty payload"
                        );
                        self.commit_message(&record);
                    }
                }
                Err(e) => {
                    consecutive_errors += 1;
                    let now = std::time::Instant::now();

                    // 检查是否应该记录错误
                    let should_log = last_error_time
                        .map(|last| now.duration_since(last).as_secs() >= 60)
                        .unwrap_or(true);

                    if should_log {
                        error!(
                            error = %e,
                            consecutive_errors,
                            "Failed to receive ACK message from Kafka"
                        );
                        last_error_time = Some(now);
                    }

                    // 如果连续错误太多，等待一段时间再重试
                    if consecutive_errors >= 10 {
                        warn!(
                            consecutive_errors,
                            "Too many consecutive errors, waiting 5 seconds before retry"
                        );
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        consecutive_errors = 0;
                    }
                }
            }
        }
    }

    /// 提交消息 offset
    fn commit_message(&self, record: &BorrowedMessage<'_>) {
        match self.consumer.commit_message(record, CommitMode::Async) {
            Ok(_) => {
                debug!(
                    topic = %record.topic(),
                    partition = record.partition(),
                    offset = record.offset(),
                    "ACK message offset committed"
                );
            }
            Err(e) => {
                error!(
                    error = %e,
                    topic = %record.topic(),
                    partition = record.partition(),
                    offset = record.offset(),
                    "Failed to commit ACK message offset"
                );
            }
        }
    }
}
