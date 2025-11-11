use std::sync::Arc;

use flare_server_core::error::{ErrorBuilder, ErrorCode, Result};
use rdkafka::config::ClientConfig;
use rdkafka::consumer::{Consumer, StreamConsumer};
use rdkafka::message::Message;
use serde_json::from_slice;
use tracing::{error, info, warn};

use crate::application::PushExecutionCommandService;
use crate::domain::models::PushDispatchTask;
use crate::infrastructure::config::PushWorkerConfig;

pub struct PushWorkerConsumer {
    config: Arc<PushWorkerConfig>,
    consumer: StreamConsumer,
    command_service: Arc<PushExecutionCommandService>,
}

impl PushWorkerConsumer {
    pub async fn new(
        config: Arc<PushWorkerConfig>,
        command_service: Arc<PushExecutionCommandService>,
    ) -> Result<Self> {
        let consumer: StreamConsumer = ClientConfig::new()
            .set("bootstrap.servers", &config.kafka_bootstrap)
            .set("group.id", &config.consumer_group)
            .set("enable.partition.eof", "false")
            .set("session.timeout.ms", "6000")
            .set("enable.auto.commit", "true")
            .create()
            .map_err(|err| {
                ErrorBuilder::new(
                    ErrorCode::ServiceUnavailable,
                    "failed to create kafka consumer",
                )
                .details(err.to_string())
                .build_error()
            })?;

        consumer.subscribe(&[&config.task_topic]).map_err(|err| {
            ErrorBuilder::new(
                ErrorCode::ServiceUnavailable,
                "failed to subscribe kafka topic",
            )
            .details(err.to_string())
            .build_error()
        })?;

        info!(
            bootstrap = %config.kafka_bootstrap,
            group = %config.consumer_group,
            topic = %config.task_topic,
            "PushWorker connected to Kafka"
        );

        Ok(Self {
            config,
            consumer,
            command_service,
        })
    }

    pub async fn run(&self) -> Result<()> {
        loop {
            match self.consumer.recv().await {
                Ok(message) => {
                    if let Some(payload) = message.payload() {
                        match from_slice::<PushDispatchTask>(payload) {
                            Ok(task) => {
                                if let Err(err) = self.command_service.execute(task).await {
                                    error!(?err, "failed to execute push task");
                                }
                            }
                            Err(err) => warn!(?err, "failed to decode push dispatch task"),
                        }
                    }
                }
                Err(err) => error!(?err, "error receiving task from Kafka"),
            }
        }
    }

    pub fn config(&self) -> &Arc<PushWorkerConfig> {
        &self.config
    }
}
