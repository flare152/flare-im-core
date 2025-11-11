use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use flare_proto::storage::StoreMessageRequest;
use prost::Message;
use rdkafka::producer::{FutureProducer, FutureRecord};

use crate::domain::repositories::MessageEventPublisher;
use crate::infrastructure::config::MessageOrchestratorConfig;

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
    async fn publish(&self, payload: StoreMessageRequest) -> Result<()> {
        let encoded = payload.encode_to_vec();

        let record = FutureRecord::to(&self.config.kafka_topic)
            .payload(&encoded)
            .key(&payload.session_id);

        self.producer
            .send(record, Duration::from_millis(self.config.kafka_timeout_ms))
            .await
            .map_err(|(err, _)| anyhow!("Failed to send to Kafka: {err}"))?;

        Ok(())
    }
}
