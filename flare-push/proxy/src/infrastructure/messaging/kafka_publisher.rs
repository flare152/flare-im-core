use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use flare_proto::push::{PushMessageRequest, PushNotificationRequest};
use prost::Message;
use rdkafka::config::ClientConfig;
use rdkafka::producer::{FutureProducer, FutureRecord};

use crate::domain::repositories::PushEventPublisher;
use crate::infrastructure::config::PushProxyConfig;

pub struct KafkaPushEventPublisher {
    config: Arc<PushProxyConfig>,
    producer: Arc<FutureProducer>,
}

impl KafkaPushEventPublisher {
    pub fn new(config: Arc<PushProxyConfig>) -> Result<Self> {
        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", &config.kafka_bootstrap)
            .set("message.timeout.ms", config.kafka_timeout_ms.to_string())
            .create()
            .map_err(|err| anyhow!("failed to create Kafka producer: {err}"))?;

        Ok(Self {
            config,
            producer: Arc::new(producer),
        })
    }
}

#[async_trait]
impl PushEventPublisher for KafkaPushEventPublisher {
    async fn publish_message(&self, request: &PushMessageRequest) -> Result<()> {
        let payload = request.encode_to_vec();
        let key = request.user_ids.first().map(|s| s.as_str()).unwrap_or("");
        let record = FutureRecord::to(&self.config.message_topic)
            .payload(&payload)
            .key(key);

        self.producer
            .send(record, Duration::from_millis(self.config.kafka_timeout_ms))
            .await
            .map_err(|(err, _)| anyhow!("failed to enqueue push message: {err}"))?;

        Ok(())
    }

    async fn publish_notification(&self, request: &PushNotificationRequest) -> Result<()> {
        let payload = request.encode_to_vec();
        let key = request.user_ids.first().map(|s| s.as_str()).unwrap_or("");
        let record = FutureRecord::to(&self.config.notification_topic)
            .payload(&payload)
            .key(key);

        self.producer
            .send(record, Duration::from_millis(self.config.kafka_timeout_ms))
            .await
            .map_err(|(err, _)| anyhow!("failed to enqueue push notification: {err}"))?;

        Ok(())
    }
}
