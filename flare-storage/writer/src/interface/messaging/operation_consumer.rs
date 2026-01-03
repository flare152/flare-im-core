use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use flare_im_core::metrics::StorageWriterMetrics;
use flare_proto::storage::StoreMessageRequest;
use flare_server_core::error::{ErrorBuilder, ErrorCode};
use flare_server_core::kafka::{build_kafka_consumer, subscribe_and_wait_for_assignment};
use prost::Message as _;
use rdkafka::consumer::{CommitMode, Consumer, StreamConsumer};
use rdkafka::message::BorrowedMessage;
use rdkafka::Message;
use tracing::{Span, debug, error, info, instrument, warn};

use crate::application::commands::ProcessMessageOperationCommand;
use crate::application::handlers::MessagePersistenceCommandHandler;
use crate::config::StorageWriterConfig;

pub struct OperationMessageConsumer {
    config: Arc<StorageWriterConfig>,
    kafka_consumer: StreamConsumer,
    command_handler: Arc<MessagePersistenceCommandHandler>,
    metrics: Arc<StorageWriterMetrics>,
}

impl OperationMessageConsumer {
    pub async fn new(
        config: Arc<StorageWriterConfig>,
        command_handler: Arc<MessagePersistenceCommandHandler>,
        metrics: Arc<StorageWriterMetrics>,
    ) -> Result<Self> {
        let consumer = build_kafka_consumer(
            config.as_ref() as &dyn flare_server_core::kafka::KafkaConsumerConfig
        )
        .map_err(|err| {
            ErrorBuilder::new(
                ErrorCode::ServiceUnavailable,
                "failed to build operation kafka consumer",
            )
            .details(err.to_string())
            .build_error()
        })?;

        info!(
            bootstrap = %config.kafka_bootstrap,
            group = %config.kafka_group,
            topic = %config.kafka_operation_topic,
            "Subscribing to operation message Kafka topic..."
        );

        subscribe_and_wait_for_assignment(&consumer, &config.kafka_operation_topic, 15)
            .await
            .map_err(|err| {
                ErrorBuilder::new(
                    ErrorCode::ServiceUnavailable,
                    "failed to subscribe or assign operation kafka topics",
                )
                .details(err)
                .build_error()
            })?;

        info!(
            bootstrap = %config.kafka_bootstrap,
            group = %config.kafka_group,
            topic = %config.kafka_operation_topic,
            "Operation message consumer initialized and ready"
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
            topic = %self.config.kafka_operation_topic,
            group_id = %self.config.kafka_group,
            "Starting operation message consumer loop"
        );

        loop {
            let mut batch = Vec::new();
            let max_records = 100;

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
                            "Received operation message from Kafka"
                        );
                        batch.push(message);
                    }
                    Ok(Err(e)) => {
                        error!(error = ?e, "Error receiving operation message from Kafka");
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        break;
                    }
                    Err(_) => {
                        debug!(
                            "Timeout waiting for operation messages, processing {} collected messages",
                            batch.len()
                        );
                        break;
                    }
                }
            }

            if !batch.is_empty() {
                if let Err(e) = self.process_batch(batch).await {
                    error!(error = ?e, "Failed to process operation message batch");
                }
            } else {
                debug!("No operation messages received, continuing to wait...");
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
            "Processing batch of {} operation messages from Kafka",
            messages.len()
        );

        self.metrics.batch_size.observe(batch_size);

        let mut valid_messages = Vec::new();

        for message in messages {
            let payload = match message.payload() {
                Some(payload) => payload,
                None => {
                    warn!("Kafka operation message without payload encountered");
                    continue;
                }
            };

            match StoreMessageRequest::decode(payload) {
                Ok(request) => {
                    if let Some(msg) = &request.message {
                        if crate::domain::service::MessageOperationDomainService::is_operation_message(msg) {
                            if let Ok(Some(operation)) = crate::domain::service::MessageOperationDomainService::extract_operation_from_message(msg) {
                                if let Err(e) = self.command_handler
                                    .handle_operation_message(ProcessMessageOperationCommand {
                                        operation,
                                        message: msg.clone(),
                                    })
                                    .await
                                {
                                    error!(error = ?e, "Failed to process operation message");
                                    continue;
                                }
                                valid_messages.push(message);
                            }
                        }
                    }
                }
                Err(err) => {
                    error!(error = ?err, "Failed to decode operation StoreMessageRequest");
                    continue;
                }
            }
        }

        let batch_duration = batch_start.elapsed();
        self.metrics
            .messages_persisted_duration_seconds
            .observe(batch_duration.as_secs_f64());

        for message in &valid_messages {
            self.commit_message(message);
        }

        info!(
            batch_size = valid_messages.len(),
            "Batch operation messages processed successfully"
        );

        Ok(())
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

