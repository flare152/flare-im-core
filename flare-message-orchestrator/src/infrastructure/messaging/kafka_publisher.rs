use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use flare_proto::storage::StoreMessageRequest;
use flare_proto::push::PushMessageRequest;
use prost::Message;
use rdkafka::producer::{FutureProducer, FutureRecord};

use crate::domain::repository::MessageEventPublisher;
use crate::config::MessageOrchestratorConfig;

pub struct KafkaMessagePublisher {
    producer: Arc<FutureProducer>,
    config: Arc<MessageOrchestratorConfig>,
}

impl KafkaMessagePublisher {
    pub fn new(producer: Arc<FutureProducer>, config: Arc<MessageOrchestratorConfig>) -> Self {
        Self { producer, config }
    }
}

#[async_trait]
impl MessageEventPublisher for KafkaMessagePublisher {
    async fn publish_storage(&self, payload: StoreMessageRequest) -> Result<()> {
        let encoded = payload.encode_to_vec();
        
        // 验证消息大小
        const MAX_MESSAGE_SIZE: usize = 10 * 1024 * 1024; // 10MB
        if encoded.len() > MAX_MESSAGE_SIZE {
            tracing::error!(
                payload_size = encoded.len(),
                max_size = MAX_MESSAGE_SIZE,
                session_id = %payload.session_id,
                message_id = payload.message.as_ref().map(|m| m.id.as_str()).unwrap_or("unknown"),
                "Storage message size exceeds maximum allowed size"
            );
            return Err(anyhow!(
                "Storage message size {} bytes exceeds maximum allowed size {} bytes",
                encoded.len(),
                MAX_MESSAGE_SIZE
            ));
        }

        let record = FutureRecord::to(&self.config.kafka_storage_topic)
            .payload(&encoded)
            .key(&payload.session_id);

        tracing::debug!(
            topic = %self.config.kafka_storage_topic,
            session_id = %payload.session_id,
            payload_size = encoded.len(),
            "Publishing storage message to Kafka"
        );

        self.producer
            .send(record, Duration::from_millis(self.config.kafka_timeout_ms))
            .await
            .map_err(|(err, _)| anyhow!("Failed to send to Kafka storage topic: {err}"))?;

        Ok(())
    }

    async fn publish_push(&self, payload: PushMessageRequest) -> Result<()> {
        let encoded = payload.encode_to_vec();
        
        // 验证消息大小，防止发送异常大的消息
        const MAX_MESSAGE_SIZE: usize = 10 * 1024 * 1024; // 10MB
        if encoded.len() > MAX_MESSAGE_SIZE {
            tracing::error!(
                payload_size = encoded.len(),
                max_size = MAX_MESSAGE_SIZE,
                "Message size exceeds maximum allowed size"
            );
            return Err(anyhow!(
                "Message size {} bytes exceeds maximum allowed size {} bytes",
                encoded.len(),
                MAX_MESSAGE_SIZE
            ));
        }

        // 使用第一个user_id作为key，如果没有则使用空字符串（聊天室消息）
        let key = payload.user_ids.first()
            .map(|s| s.as_str())
            .unwrap_or("");

        let record = FutureRecord::to(&self.config.kafka_push_topic)
            .payload(&encoded)
            .key(key);

        tracing::info!(
            topic = %self.config.kafka_push_topic,
            key = %key,
            payload_size = encoded.len(),
            user_ids_count = payload.user_ids.len(),
            user_ids = ?payload.user_ids,
            "Publishing push message to Kafka"
        );

        match self.producer
            .send(record, Duration::from_millis(self.config.kafka_timeout_ms))
            .await
        {
            Ok(_delivery_result) => {
                tracing::info!(
                    topic = %self.config.kafka_push_topic,
                    "Successfully published push message to Kafka"
                );
                Ok(())
            }
            Err((err, _)) => {
                tracing::error!(
                    error = %err,
                    topic = %self.config.kafka_push_topic,
                    "Failed to send to Kafka push topic"
                );
                Err(anyhow!("Failed to send to Kafka push topic: {err}"))
            }
        }
    }

    async fn publish_both(
        &self,
        storage_payload: StoreMessageRequest,
        push_payload: PushMessageRequest,
    ) -> Result<()> {
        // 并行发布到两个topic
        let (storage_result, push_result) = tokio::join!(
            self.publish_storage(storage_payload),
            self.publish_push(push_payload)
        );

        storage_result?;
        push_result?;

        Ok(())
    }
}
