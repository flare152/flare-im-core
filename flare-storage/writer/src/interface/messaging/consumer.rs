use std::sync::Arc;
use std::time::Instant;

use anyhow::Result as AnyhowResult;
use anyhow::Result;
use flare_im_core::metrics::StorageWriterMetrics;
#[cfg(feature = "tracing")]
use flare_im_core::tracing::{create_span, set_error, set_message_id, set_tenant_id};
use flare_proto::storage::StoreMessageRequest;
use flare_server_core::error::{ErrorBuilder, ErrorCode};
use flare_server_core::kafka::{build_kafka_consumer, subscribe_and_wait_for_assignment};
use prost::Message as _;
use rdkafka::Message;
use rdkafka::consumer::{CommitMode, Consumer, StreamConsumer};
use rdkafka::message::BorrowedMessage;
use tracing::{Span, debug, error, info, instrument, warn};

use crate::application::commands::ProcessStoreMessageCommand;
use crate::application::handlers::MessagePersistenceCommandHandler;
use crate::config::StorageWriterConfig;
use crate::domain::model::PersistenceResult;

pub struct StorageWriterConsumer {
    config: Arc<StorageWriterConfig>,
    kafka_consumer: StreamConsumer,
    command_handler: Arc<MessagePersistenceCommandHandler>,
    metrics: Arc<StorageWriterMetrics>,
}

impl StorageWriterConsumer {
    /// 创建新的 StorageWriterConsumer
    ///
    /// 使用统一的 Kafka 消费者构建器（从 flare-server-core）
    /// 与 push-server 的构建方式完全一致
    pub async fn new(
        config: Arc<StorageWriterConfig>,
        command_handler: Arc<MessagePersistenceCommandHandler>,
        metrics: Arc<StorageWriterMetrics>,
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
            group = %config.kafka_group,
            topic = %config.kafka_topic,
            "Subscribing to Kafka topic..."
        );

        // 订阅并等待 partition assignment（最多等待 15 秒）
        subscribe_and_wait_for_assignment(&consumer, &config.kafka_topic, 15)
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
            group = %config.kafka_group,
            topic = %config.kafka_topic,
            "StorageWriter Kafka Consumer initialized and ready"
        );

        Ok(Self {
            config,
            kafka_consumer: consumer,
            command_handler,
            metrics,
        })
    }

    pub async fn consume_messages(&self) -> Result<(), Box<dyn std::error::Error>> {
        info!(
            topic = %self.config.kafka_topic,
            group_id = %self.config.kafka_group,
            "Starting Kafka consumer loop"
        );

        loop {
            // 批量消费消息
            let mut batch = Vec::new();

            // 收集一批消息（最多100条，或等待fetch_max_wait_ms）
            let max_records = 100;
            debug!(
                "Waiting for messages from Kafka (timeout: {}ms)",
                self.config.fetch_max_wait_ms
            );

            for _ in 0..max_records {
                match tokio::time::timeout(
                    std::time::Duration::from_millis(self.config.fetch_max_wait_ms),
                    self.kafka_consumer.recv(),
                )
                .await
                {
                    Ok(Ok(message)) => {
                        debug!(
                            partition = message.partition(),
                            offset = message.offset(),
                            "Received message from Kafka"
                        );
                        batch.push(message);
                    }
                    Ok(Err(e)) => {
                        error!(error = ?e, "Error receiving message from Kafka");
                        // 添加错误重试机制，避免消费者因临时错误而终止
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        break; // 跳出循环，处理已收集的消息
                    }
                    Err(_) => {
                        // 超时，处理已收集的消息
                        debug!(
                            "Timeout waiting for messages, processing {} collected messages",
                            batch.len()
                        );
                        break;
                    }
                }
            }

            // 批量处理消息
            if !batch.is_empty() {
                info!(
                    batch_size = batch.len(),
                    "Processing batch of {} messages from Kafka",
                    batch.len()
                );
                if let Err(e) = self.process_batch(batch).await {
                    error!(error = ?e, "Failed to process batch");
                }
            } else {
                // 没有消息时，记录一次调试日志（避免日志过多）
                debug!("No messages received, continuing to wait...");
            }
        }
    }

    #[instrument(skip(self), fields(batch_size))]
    async fn process_batch(
        &self,
        messages: Vec<BorrowedMessage<'_>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let batch_start = Instant::now();
        let batch_size = messages.len() as f64;
        let span = Span::current();
        span.record("batch_size", batch_size as u64);

        info!(
            batch_size = messages.len(),
            "Processing batch of {} messages from Kafka",
            messages.len()
        );

        // 记录批量大小
        self.metrics.batch_size.observe(batch_size);

        let mut requests = Vec::new();
        let mut valid_messages = Vec::new();

        // 解析所有消息
        for message in messages {
            let payload = match message.payload() {
                Some(payload) => payload,
                None => {
                    warn!("Kafka message without payload encountered");
                    continue;
                }
            };

            match StoreMessageRequest::decode(payload) {
                Ok(mut request) => {
                    // 即使解码成功，也要清理字符串字段，确保所有字段都是有效的 UTF-8
                    // 这是为了避免后续处理中的编码问题
                    if let Some(ref mut msg) = request.message {
                        msg.client_msg_id =
                            String::from_utf8_lossy(msg.client_msg_id.as_bytes()).to_string();
                        if let Some(ref mut content) = msg.content {
                            if let Some(flare_proto::common::message_content::Content::Text(
                                ref mut text_content,
                            )) = content.content
                            {
                                text_content.text =
                                    String::from_utf8_lossy(text_content.text.as_bytes())
                                        .to_string();
                            }
                        }
                    }
                    requests.push(request);
                    valid_messages.push(message);
                }
                Err(err) => {
                    // 兼容：若收到的是 PushMessageRequest，尝试转换为 StoreMessageRequest
                    tracing::warn!(error = ?err, "Failed to decode StoreMessageRequest, trying PushMessageRequest fallback");
                    if let Ok(mut push_req) = flare_proto::push::PushMessageRequest::decode(payload)
                    {
                        if let Some(ref mut msg) = push_req.message {
                            msg.client_msg_id =
                                String::from_utf8_lossy(msg.client_msg_id.as_bytes()).to_string();
                            if let Some(ref mut content) = msg.content {
                                if let Some(flare_proto::common::message_content::Content::Text(
                                    ref mut text_content,
                                )) = content.content
                                {
                                    text_content.text =
                                        String::from_utf8_lossy(text_content.text.as_bytes())
                                            .to_string();
                                }
                            }
                            let store = flare_proto::storage::StoreMessageRequest {
                                session_id: msg.session_id.clone(),
                                message: Some(msg.clone()),
                                sync: false,
                                context: Default::default(),
                                tenant: Default::default(),
                                tags: std::collections::HashMap::new(),
                            };
                            requests.push(store);
                            valid_messages.push(message);
                        } else {
                            error!("PushMessageRequest without message payload");
                        }
                    } else {
                        // 开发阶段：直接跳过无法解码的旧消息，记录警告但不阻塞处理
                        tracing::warn!(
                            error = ?err,
                            offset = message.offset(),
                            partition = message.partition(),
                            "Failed to decode message, skipping (development mode)"
                        );
                        // 不添加到 valid_messages，这样 offset 不会被提交，但也不会阻塞其他消息的处理
                        // 在开发阶段，我们可以手动跳过这些消息
                    }
                }
            }
        }

        // 批量处理消息（优化性能）
        let commands: Vec<_> = requests
            .into_iter()
            .map(|req| ProcessStoreMessageCommand { request: req })
            .collect();

        if let Err(e) = self.command_handler.handle_batch(commands).await {
            error!(error = %e, "Failed to process batch");
            // 不提交 offset，等待后续重试
            return Ok(());
        }

        // 记录批量处理耗时
        let batch_duration = batch_start.elapsed();
        self.metrics
            .messages_persisted_duration_seconds
            .observe(batch_duration.as_secs_f64());

        // 提交所有消息的offset
        for message in &valid_messages {
            self.commit_message(message);
        }

        // 记录批量处理成功
        info!(
            batch_size = valid_messages.len(),
            "Batch messages persisted successfully"
        );

        Ok(())
    }

    async fn process_store_message(
        &self,
        request: StoreMessageRequest,
    ) -> AnyhowResult<PersistenceResult> {
        // 检查是否是操作消息
        if let Some(message) = &request.message {
            if crate::domain::service::MessageOperationDomainService::is_operation_message(message)
            {
                // 提取 MessageOperation
                if let Ok(Some(operation)) = crate::domain::service::MessageOperationDomainService::extract_operation_from_message(message) {
                    // 处理操作消息
                    use crate::application::commands::ProcessMessageOperationCommand;
                    return self.command_handler
                        .handle_operation_message(ProcessMessageOperationCommand {
                            operation,
                            message: message.clone(),
                        })
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to process message operation: {}", e));
                }
            }
        }

        // 普通消息处理
        self.command_handler
            .handle(ProcessStoreMessageCommand { request })
            .await
    }

    fn commit_message(&self, message: &BorrowedMessage<'_>) {
        if let Err(err) = self
            .kafka_consumer
            .commit_message(message, CommitMode::Async)
        {
            warn!(error = ?err, "Failed to commit Kafka message");
        }
    }
}
