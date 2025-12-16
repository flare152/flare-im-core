use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use flare_server_core::error::{ErrorBuilder, ErrorCode, Result};
use flare_server_core::kafka::build_kafka_producer;
use rdkafka::producer::{FutureProducer, FutureRecord};
use serde_json::{json, to_vec};

use crate::config::PushServerConfig;
use crate::domain::model::PushDispatchTask;
use crate::domain::repository::PushTaskPublisher;

pub struct KafkaPushTaskPublisher {
    config: Arc<PushServerConfig>,
    producer: Arc<FutureProducer>,
}

impl KafkaPushTaskPublisher {
    pub fn new(config: Arc<PushServerConfig>) -> Result<Self> {
        // 使用统一的 Kafka 生产者构建器（从 flare-server-core）
        let producer = build_kafka_producer(
            config.as_ref() as &dyn flare_server_core::kafka::KafkaProducerConfig
        )
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

    async fn publish_offline_batch(&self, tasks: &[PushDispatchTask]) -> Result<()> {
        let timeout = Duration::from_millis(self.config.kafka_timeout_ms);

        // 批量发送离线推送任务
        for task in tasks {
            let payload = to_vec(task).map_err(|err| {
                ErrorBuilder::new(
                    ErrorCode::SerializationError,
                    "failed to encode offline task",
                )
                .details(err.to_string())
                .build_error()
            })?;

            let record = FutureRecord::to(&self.config.offline_topic)
                .payload(&payload)
                .key(&task.user_id);

            self.producer
                .send(record, timeout)
                .await
                .map_err(|(err, _)| {
                    ErrorBuilder::new(
                        ErrorCode::ServiceUnavailable,
                        "failed to enqueue offline task",
                    )
                    .details(err.to_string())
                    .build_error()
                })?;
        }

        Ok(())
    }

    async fn publish_to_dlq(
        &self,
        task: &PushDispatchTask,
        error: &str,
        retry_count: u32,
    ) -> Result<()> {
        // 构建死信队列记录
        let dlq_record = json!({
            "task": task,
            "error": error,
            "retry_count": retry_count,
            "failed_at": chrono::Utc::now().to_rfc3339(),
        });

        let payload = to_vec(&dlq_record).map_err(|err| {
            ErrorBuilder::new(ErrorCode::SerializationError, "failed to encode dlq record")
                .details(err.to_string())
                .build_error()
        })?;

        let record = FutureRecord::to(&self.config.dlq_topic)
            .payload(&payload)
            .key(&task.message_id);

        self.producer
            .send(record, Duration::from_millis(self.config.kafka_timeout_ms))
            .await
            .map_err(|(err, _)| {
                ErrorBuilder::new(
                    ErrorCode::ServiceUnavailable,
                    "failed to enqueue dlq record",
                )
                .details(err.to_string())
                .build_error()
            })?;

        Ok(())
    }
}
