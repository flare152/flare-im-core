//! 死信队列发布器（基础设施层实现）

use async_trait::async_trait;
use flare_server_core::error::{ErrorBuilder, ErrorCode, Result};
use flare_server_core::kafka::build_kafka_producer;
use rdkafka::producer::{FutureProducer, FutureRecord};
use serde_json;
use std::sync::Arc;
use tracing::{error, info};

use crate::domain::model::PushDispatchTask;

/// Kafka死信队列发布器
pub struct KafkaDlqPublisher {
    producer: FutureProducer,
    topic: String,
}

impl KafkaDlqPublisher {
    pub fn new(bootstrap_servers: &str, topic: String) -> Result<Arc<Self>> {
        // 创建简单的配置包装器
        struct SimpleProducerConfig {
            bootstrap: String,
        }

        impl flare_server_core::kafka::KafkaProducerConfig for SimpleProducerConfig {
            fn kafka_bootstrap(&self) -> &str {
                &self.bootstrap
            }

            fn message_timeout_ms(&self) -> u64 {
                5000 // 默认 5 秒
            }
        }

        let config = SimpleProducerConfig {
            bootstrap: bootstrap_servers.to_string(),
        };

        // 使用统一的 Kafka 生产者构建器（从 flare-server-core）
        let producer =
            build_kafka_producer(&config as &dyn flare_server_core::kafka::KafkaProducerConfig)
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
impl crate::domain::repository::DlqPublisher for KafkaDlqPublisher {
    async fn publish_to_dlq(&self, task: &PushDispatchTask, error: &str) -> Result<()> {
        // 构建死信消息（包含原始任务和错误信息）
        let dlq_message = serde_json::json!({
            "task": task,
            "error": error,
            "timestamp": chrono::Utc::now().timestamp(),
        });

        let payload = serde_json::to_vec(&dlq_message).map_err(|e| {
            ErrorBuilder::new(ErrorCode::InternalError, "Failed to serialize DLQ")
                .details(e.to_string())
                .build_error()
        })?;

        let record = FutureRecord::to(&self.topic)
            .key(&task.message_id)
            .payload(&payload);

        match self
            .producer
            .send(record, std::time::Duration::from_secs(0))
            .await
        {
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
                Err(
                    ErrorBuilder::new(ErrorCode::ServiceUnavailable, "Failed to publish to DLQ")
                        .details(e.to_string())
                        .build_error(),
                )
            }
        }
    }
}
