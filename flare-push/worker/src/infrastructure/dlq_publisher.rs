//! 死信队列发布器

use async_trait::async_trait;
use flare_server_core::error::{ErrorBuilder, ErrorCode, Result};
use rdkafka::producer::{FutureProducer, FutureRecord};
use rdkafka::ClientConfig;
use serde_json;
use std::sync::Arc;
use tracing::{error, info};

use crate::domain::models::PushDispatchTask;

/// 死信队列发布器trait
#[async_trait]
pub trait DlqPublisher: Send + Sync {
    async fn publish_to_dlq(&self, task: &PushDispatchTask, error: &str) -> Result<()>;
}

/// Kafka死信队列发布器
pub struct KafkaDlqPublisher {
    producer: FutureProducer,
    topic: String,
}

impl KafkaDlqPublisher {
    pub fn new(bootstrap_servers: &str, topic: String) -> Result<Arc<Self>> {
        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", bootstrap_servers)
            .set("message.timeout.ms", "5000")
            .create()
            .map_err(|e| {
                ErrorBuilder::new(
                    ErrorCode::ServiceUnavailable,
                    "Failed to create Kafka producer",
                )
                .details(e.to_string())
                .build_error()
            })?;

        Ok(Arc::new(Self { producer, topic }))
    }
}

#[async_trait]
impl DlqPublisher for KafkaDlqPublisher {
    async fn publish_to_dlq(&self, task: &PushDispatchTask, error: &str) -> Result<()> {
        // 构建死信消息（包含原始任务和错误信息）
        let dlq_message = serde_json::json!({
            "task": task,
            "error": error,
            "timestamp": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
        });

        let payload = serde_json::to_vec(&dlq_message).map_err(|e| {
            ErrorBuilder::new(
                ErrorCode::InternalError,
                "Failed to serialize DLQ",
            )
            .details(e.to_string())
            .build_error()
        })?;

        let record = FutureRecord::to(&self.topic)
            .key(&task.message_id)
            .payload(&payload);

        match self.producer.send(record, std::time::Duration::from_secs(0)).await {
            Ok(_) => {
                info!(
                    message_id = %task.message_id,
                    user_id = %task.user_id,
                    ?error,
                    "Task sent to DLQ"
                );
                Ok(())
            }
            Err((e, _)) => {
                error!(
                    message_id = %task.message_id,
                    ?e,
                    "Failed to publish to DLQ"
                );
                Err(ErrorBuilder::new(
                    ErrorCode::ServiceUnavailable,
                    "Failed to publish to DLQ",
                )
                .details(e.to_string())
                .build_error())
            }
        }
    }
}

