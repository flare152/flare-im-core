use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use flare_server_core::error::{ErrorBuilder, ErrorCode, Result};
use rdkafka::config::ClientConfig;
use rdkafka::producer::{FutureProducer, FutureRecord};
use serde_json::to_vec;

use crate::domain::models::PushDispatchTask;
use crate::domain::repositories::PushTaskPublisher;
use crate::infrastructure::config::PushServerConfig;

pub struct KafkaPushTaskPublisher {
    config: Arc<PushServerConfig>,
    producer: Arc<FutureProducer>,
}

impl KafkaPushTaskPublisher {
    pub fn new(config: Arc<PushServerConfig>) -> Result<Self> {
        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", &config.kafka_bootstrap)
            .set("message.timeout.ms", "5000")
            .create()
            .map_err(|err| {
                ErrorBuilder::new(
                    ErrorCode::ServiceUnavailable,
                    "failed to create kafka producer",
                )
                .details(err.to_string())
                .build_error()
            })?;

        Ok(Self {
            config,
            producer: Arc::new(producer),
        })
    }
}

#[async_trait]
impl PushTaskPublisher for KafkaPushTaskPublisher {
    async fn publish(&self, task: &PushDispatchTask) -> Result<()> {
        let payload = to_vec(task).map_err(|err| {
            ErrorBuilder::new(
                ErrorCode::SerializationError,
                "failed to encode dispatch task",
            )
            .details(err.to_string())
            .build_error()
        })?;

        let record = FutureRecord::to(&self.config.task_topic)
            .payload(&payload)
            .key(&task.user_id);

        self.producer
            .send(record, Duration::from_millis(self.config.kafka_timeout_ms))
            .await
            .map_err(|(err, _)| {
                ErrorBuilder::new(ErrorCode::ServiceUnavailable, "failed to enqueue push task")
                    .details(err.to_string())
                    .build_error()
            })?;

        Ok(())
    }
}
