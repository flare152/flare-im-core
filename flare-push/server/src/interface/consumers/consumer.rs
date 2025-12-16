use std::sync::Arc;
use std::time::Duration;

use flare_im_core::metrics::PushServerMetrics;
use flare_proto::push::PushMessageRequest;
use flare_server_core::error::{ErrorBuilder, ErrorCode, Result};
use prost::Message;
use rdkafka::consumer::{CommitMode, Consumer, StreamConsumer};
use rdkafka::message::{BorrowedMessage, Message as _};
use tracing::{debug, error, info, warn};

use crate::application::commands::PushMessageCommand;
use crate::application::handlers::PushCommandHandler;
use crate::config::PushServerConfig;
use flare_server_core::kafka::{
    KafkaConsumerConfig, build_kafka_consumer, subscribe_and_wait_for_assignment,
};

pub struct PushKafkaConsumer {
    config: Arc<PushServerConfig>,
    consumer: StreamConsumer,
    command_handler: Arc<PushCommandHandler>,
    metrics: Arc<PushServerMetrics>,
}

impl PushKafkaConsumer {
    pub async fn new(
        config: Arc<PushServerConfig>,
        command_handler: Arc<PushCommandHandler>,
        metrics: Arc<PushServerMetrics>,
    ) -> Result<Self> {
        // 使用统一的消费者构建器（从 flare-server-core）
        let consumer = build_kafka_consumer(
            config.as_ref() as &dyn flare_server_core::kafka::KafkaConsumerConfig
        )
        .map_err(|err| {
            ErrorBuilder::new(
                ErrorCode::ServiceUnavailable,
                "failed to build kafka consumer",
            )
            .details(err.to_string())
            .build_error()
        })?;

        info!(
            bootstrap = %config.kafka_bootstrap,
            group = %config.consumer_group,
            task_topic = %config.task_topic,
            "Subscribing to Kafka topic..."
        );

        // 订阅并等待 partition assignment（最多等待 15 秒）
        subscribe_and_wait_for_assignment(&consumer, &config.task_topic, 15)
            .await
            .map_err(|err| {
                ErrorBuilder::new(
                    ErrorCode::ServiceUnavailable,
                    "failed to subscribe or assign kafka topics",
                )
                .details(err)
                .build_error()
            })?;

        info!(
            bootstrap = %config.kafka_bootstrap,
            group = %config.consumer_group,
            task_topic = %config.task_topic,
            max_poll_records = config.max_poll_records,
            "PushServer Kafka Consumer initialized and ready"
        );

        Ok(Self {
            config,
            consumer,
            command_handler,
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

        // 注意：partition assignment 已在 new() 方法中完成，这里直接开始消费
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

                                // 处理单条消息（添加超时保护，避免阻塞 consumer）
                                let command = PushMessageCommand { request };
                                let handler = self.command_handler.clone();
                                let timeout_duration = std::time::Duration::from_secs(30); // 30秒超时

                                match tokio::time::timeout(
                                    timeout_duration,
                                    handler.handle_push_message(command),
                                )
                                .await
                                {
                                    Ok(Ok(_)) => {
                                        info!("Successfully processed push message");
                                        // 处理成功，提交 offset
                                        self.commit_message(&record);
                                    }
                                    Ok(Err(err)) => {
                                        error!(?err, "failed to process push message");
                                        // 处理失败时也提交 offset，避免无限重试导致 consumer 卡住
                                        // 注意：这会导致消息丢失，但可以避免整个 consumer 停止工作
                                        // 可以考虑将来发送到死信队列
                                        warn!(
                                            "Processing failed, committing offset to avoid blocking consumer"
                                        );
                                        self.commit_message(&record);
                                    }
                                    Err(_) => {
                                        error!(
                                            timeout_secs = timeout_duration.as_secs(),
                                            "push message processing timed out, skipping message"
                                        );
                                        // 超时时提交 offset，避免 consumer 卡住
                                        self.commit_message(&record);
                                    }
                                }
                            }
                            Err(err) => {
                                error!(
                                    error = ?err,
                                    offset = record.offset(),
                                    partition = record.partition(),
                                    "failed to decode PushMessageRequest, skipping message"
                                );
                                // 解码失败时，提交 offset 并跳过消息，避免 consumer 卡住
                                // 注意：这种情况下消息会丢失，但可以避免整个 consumer 停止工作
                                // 可以考虑将来发送到死信队列
                                self.commit_message(&record);
                            }
                        }
                    } else {
                        warn!("Received message with empty payload");
                        // 空 payload 的消息也需要提交 offset，避免卡住
                        self.commit_message(&record);
                    }
                }
                Err(err) => {
                    consecutive_errors += 1;
                    let now = std::time::Instant::now();

                    // 记录错误详情
                    if consecutive_errors == 1
                        || last_error_time.map_or(true, |t| now.duration_since(t).as_secs() >= 5)
                    {
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

    /// 提交 Kafka message offset
    /// 只有在手动提交模式下才需要调用此方法
    fn commit_message(&self, message: &BorrowedMessage<'_>) {
        // 只有在手动提交模式下才提交
        if !self.config.enable_auto_commit() {
            if let Err(err) = self.consumer.commit_message(message, CommitMode::Async) {
                warn!(
                    error = ?err,
                    offset = message.offset(),
                    partition = message.partition(),
                    "Failed to commit Kafka message offset"
                );
            } else {
                debug!(
                    offset = message.offset(),
                    partition = message.partition(),
                    "Committed Kafka message offset"
                );
            }
        }
    }
}
