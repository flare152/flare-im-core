use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use flare_im_core::metrics::StorageWriterMetrics;
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

pub struct NormalMessageConsumer {
    config: Arc<StorageWriterConfig>,
    kafka_consumer: StreamConsumer,
    command_handler: Arc<MessagePersistenceCommandHandler>,
    metrics: Arc<StorageWriterMetrics>,
}

impl NormalMessageConsumer {
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
                "failed to build kafka consumer",
            )
            .details(err.to_string())
            .build_error()
        })?;

        info!(
            bootstrap = %config.kafka_bootstrap,
            group = %config.kafka_group,
            topic = %config.kafka_topic,
            "Subscribing to normal message Kafka topic..."
        );

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
            "Normal message consumer initialized and ready"
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
            "Starting normal message consumer loop"
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
                            "Received normal message from Kafka"
                        );
                        batch.push(message);
                    }
                    Ok(Err(e)) => {
                        error!(error = ?e, "Error receiving normal message from Kafka");
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        break;
                    }
                    Err(_) => {
                        debug!(
                            "Timeout waiting for normal messages, processing {} collected messages",
                            batch.len()
                        );
                        break;
                    }
                }
            }

            if !batch.is_empty() {
                info!(
                    batch_size = batch.len(),
                    "Calling process_batch for {} messages",
                    batch.len()
                );
                if let Err(e) = self.process_batch(batch).await {
                    error!(error = ?e, "Failed to process normal message batch");
                }
            } else {
                debug!("No normal messages received, continuing to wait...");
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
            "Processing batch of {} normal messages from Kafka",
            messages.len()
        );

        self.metrics.batch_size.observe(batch_size);

        let mut requests = Vec::new();
        let mut valid_messages = Vec::new();

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
                                conversation_id: msg.conversation_id.clone(),
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
                        tracing::warn!(
                            error = ?err,
                            offset = message.offset(),
                            partition = message.partition(),
                            "Failed to decode message, skipping (development mode)"
                        );
                    }
                }
            }
        }

        let commands: Vec<_> = requests
            .into_iter()
            .map(|req| ProcessStoreMessageCommand { request: req })
            .collect();

        if let Err(e) = self.command_handler.handle_batch(commands).await {
            error!(error = %e, "Failed to process batch");
            return Ok(());
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
            "Batch normal messages persisted successfully"
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

