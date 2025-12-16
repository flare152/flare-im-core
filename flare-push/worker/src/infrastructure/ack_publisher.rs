//! ACK上报发布器（基础设施层实现）

use async_trait::async_trait;
use flare_server_core::error::{ErrorBuilder, ErrorCode, Result};
use flare_server_core::kafka::build_kafka_producer;
use rdkafka::producer::{FutureProducer, FutureRecord};
use serde_json;
use std::sync::Arc;
use tracing::{error, info};

use crate::domain::repository::PushAckEvent;

/// Kafka ACK发布器
pub struct KafkaAckPublisher {
    producer: FutureProducer,
    topic: String,
}

impl KafkaAckPublisher {
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
impl crate::domain::repository::AckPublisher for KafkaAckPublisher {
    async fn publish_ack(&self, event: &PushAckEvent) -> Result<()> {
        let payload = serde_json::to_vec(event).map_err(|e| {
            ErrorBuilder::new(ErrorCode::InternalError, "Failed to serialize ACK")
                .details(e.to_string())
                .build_error()
        })?;

        let record = FutureRecord::to(&self.topic)
            .key(&event.message_id)
            .payload(&payload);

        match self
            .producer
            .send(record, std::time::Duration::from_secs(0))
            .await
        {
            Ok(_) => {
                info!(
                    message_id = %event.message_id,
                    user_id = %event.user_id,
                    success = event.success,
                    "ACK published"
                );
                Ok(())
            }
            Err((e, _)) => {
                error!(
                    message_id = %event.message_id,
                    ?e,
                    "Failed to publish ACK"
                );
                Err(
                    ErrorBuilder::new(ErrorCode::ServiceUnavailable, "Failed to publish ACK")
                        .details(e.to_string())
                        .build_error(),
                )
            }
        }
    }
}

/// Noop ACK发布器（用于测试或禁用ACK上报）
pub struct NoopAckPublisher;

#[async_trait]
impl crate::domain::repository::AckPublisher for NoopAckPublisher {
    async fn publish_ack(&self, _event: &PushAckEvent) -> Result<()> {
        // 不做任何操作
        Ok(())
    }
}
