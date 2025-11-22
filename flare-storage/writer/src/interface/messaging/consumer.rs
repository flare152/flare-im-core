use std::sync::Arc;
use std::time::Instant;

use anyhow::Result as AnyhowResult;
use flare_im_core::metrics::StorageWriterMetrics;
#[cfg(feature = "tracing")]
use flare_im_core::tracing::{create_span, set_message_id, set_tenant_id, set_error};
use flare_proto::storage::StoreMessageRequest;
use prost::Message as _;
use rdkafka::consumer::{CommitMode, Consumer, StreamConsumer};
use rdkafka::message::BorrowedMessage;
use rdkafka::Message;
use tracing::{debug, error, info, warn, instrument, Span};
use anyhow::{Context, Result};
use flare_server_core::error::{ErrorBuilder, ErrorCode};
use flare_server_core::kafka::{build_kafka_consumer, subscribe_and_wait_for_assignment};

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
        let consumer = build_kafka_consumer(config.as_ref() as &dyn flare_server_core::kafka::KafkaConsumerConfig)
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
            debug!("Waiting for messages from Kafka (timeout: {}ms)", self.config.fetch_max_wait_ms);
            
            for _ in 0..max_records {
                match tokio::time::timeout(
                    std::time::Duration::from_millis(self.config.fetch_max_wait_ms),
                    self.kafka_consumer.recv(),
                ).await {
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
                        return Err(Box::new(e));
                    }
                    Err(_) => {
                        // 超时，处理已收集的消息
                        debug!("Timeout waiting for messages, processing {} collected messages", batch.len());
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
    async fn process_batch(&self, messages: Vec<BorrowedMessage<'_>>) -> Result<(), Box<dyn std::error::Error>> {
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
                Ok(request) => {
                    requests.push(request);
                    valid_messages.push(message);
                }
                Err(err) => {
                    error!(error = ?err, "Failed to decode StoreMessageRequest");
                }
            }
        }
        
        // 并发处理消息
        let mut handles = Vec::new();
        for request in requests {
            let handler = Arc::clone(&self.command_handler);
            handles.push(tokio::spawn(async move {
                handler.handle(ProcessStoreMessageCommand { request }).await
            }));
        }
        
        // 等待所有任务完成
        let mut results = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(Ok(result)) => {
                    results.push(Ok(result));
                }
                Ok(Err(e)) => {
                    results.push(Err(e));
                }
                Err(e) => {
                    error!(error = ?e, "Task join error");
                }
            }
        }
        
        // 根据处理结果决定是否提交offset
        // 如果所有消息都成功，提交所有offset；否则不提交，等待重试
        let all_success = results.iter().all(|r| r.is_ok());
        
        // 记录批量处理耗时
        let batch_duration = batch_start.elapsed();
        self.metrics.messages_persisted_duration_seconds.observe(batch_duration.as_secs_f64());
        
        if all_success {
            // 提交所有消息的offset
            for message in &valid_messages {
                self.commit_message(message);
            }
            
            // 记录批量处理成功
            info!(
                batch_size = valid_messages.len(),
                "Batch messages persisted successfully"
            );
        } else {
            // 有失败的消息，不提交offset，等待重试
            let success_count = results.iter().filter(|r| r.is_ok()).count();
            warn!(
                batch_size = valid_messages.len(),
                success_count = success_count,
                "Some messages failed; keeping offset for retry"
            );
        }
        
        Ok(())
    }

    async fn process_store_message(
        &self,
        request: StoreMessageRequest,
    ) -> AnyhowResult<PersistenceResult> {
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
