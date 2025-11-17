//! ACK上报发布器

use async_trait::async_trait;
use flare_server_core::error::{ErrorBuilder, ErrorCode, Result};
use rdkafka::producer::{FutureProducer, FutureRecord};
use rdkafka::ClientConfig;
use serde_json;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{error, info};

/// ACK事件
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PushAckEvent {
    pub message_id: String,
    pub user_id: String,
    pub connection_id: String,
    pub gateway_id: String,
    pub ack_type: String, // "client_ack" | "push_ack"
    pub status: String,   // "success" | "failed"
    pub timestamp: i64,
}

/// ACK发布器trait
#[async_trait]
pub trait AckPublisher: Send + Sync {
    async fn publish_ack(&self, event: &PushAckEvent) -> Result<()>;
}

/// Kafka ACK发布器
pub struct KafkaAckPublisher {
    producer: Arc<FutureProducer>,
    topic: String,
}

impl KafkaAckPublisher {
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

        Ok(Arc::new(Self {
            producer: Arc::new(producer),
            topic,
        }))
    }
}

#[async_trait]
impl AckPublisher for KafkaAckPublisher {
    async fn publish_ack(&self, event: &PushAckEvent) -> Result<()> {
        let payload = serde_json::to_vec(event).map_err(|e| {
            ErrorBuilder::new(
                ErrorCode::InternalError,
                "Failed to serialize ACK",
            )
            .details(e.to_string())
            .build_error()
        })?;

        let record = FutureRecord::to(&self.topic)
            .key(&event.message_id)
            .payload(&payload);

        match self.producer.send(record, std::time::Duration::from_secs(5)).await {
            Ok(_) => {
                info!(
                    message_id = %event.message_id,
                    user_id = %event.user_id,
                    ack_type = %event.ack_type,
                    status = %event.status,
                    "Client ACK published"
                );
                Ok(())
            }
            Err((e, _)) => {
                tracing::error!(
                    message_id = %event.message_id,
                    user_id = %event.user_id,
                    ?e,
                    "Failed to publish client ACK"
                );
                Err(ErrorBuilder::new(
                    ErrorCode::ServiceUnavailable,
                    "Failed to publish client ACK",
                )
                .details(e.to_string())
                .build_error())
            }
        }
    }
}

/// Noop ACK发布器（用于测试或禁用ACK上报）
pub struct NoopAckPublisher;

#[async_trait]
impl AckPublisher for NoopAckPublisher {
    async fn publish_ack(&self, _event: &PushAckEvent) -> Result<()> {
        // 不做任何操作
        Ok(())
    }
}

