use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, anyhow};
use rdkafka::producer::{FutureProducer, FutureRecord};
use serde_json::to_vec;

use crate::config::StorageWriterConfig;
use crate::domain::events::AckEvent;
use crate::domain::repository::AckPublisher;

pub struct KafkaAckPublisher {
    producer: Arc<FutureProducer>,
    config: Arc<StorageWriterConfig>,
    topic: String,
}

impl KafkaAckPublisher {
    pub fn new(
        producer: Arc<FutureProducer>,
        config: Arc<StorageWriterConfig>,
        topic: String,
    ) -> Self {
        Self {
            producer,
            config,
            topic,
        }
    }
}

#[async_trait]
impl AckPublisher for KafkaAckPublisher {
    async fn publish(&self, event: AckEvent<'_>) -> Result<()> {
        let payload = to_vec(&event)?;

        let record = FutureRecord::to(&self.topic)
            .payload(&payload)
            .key(event.conversation_id);

        self.producer
            .send(record, Duration::from_millis(self.config.kafka_timeout_ms))
            .await
            .map_err(|(err, _)| anyhow!("failed to publish ACK: {err}"))?;

        Ok(())
    }
}
