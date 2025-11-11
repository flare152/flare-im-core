use std::sync::Arc;

use flare_proto::push::{PushMessageRequest, PushNotificationRequest};
use flare_server_core::error::{ErrorBuilder, ErrorCode, Result};
use prost::Message;
use rdkafka::config::ClientConfig;
use rdkafka::consumer::{Consumer, StreamConsumer};
use rdkafka::message::Message as _;
use tracing::{error, info, warn};

use crate::application::PushDispatchCommandService;
use crate::infrastructure::config::PushServerConfig;

pub struct PushKafkaConsumer {
    config: Arc<PushServerConfig>,
    consumer: StreamConsumer,
    command_service: Arc<PushDispatchCommandService>,
}

impl PushKafkaConsumer {
    pub async fn new(
        config: Arc<PushServerConfig>,
        command_service: Arc<PushDispatchCommandService>,
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
                    "failed to build kafka consumer",
                )
                .details(err.to_string())
                .build_error()
            })?;

        consumer
            .subscribe(&[&config.message_topic, &config.notification_topic])
            .map_err(|err| {
                ErrorBuilder::new(
                    ErrorCode::ServiceUnavailable,
                    "failed to subscribe kafka topics",
                )
                .details(err.to_string())
                .build_error()
            })?;

        info!(
            bootstrap = %config.kafka_bootstrap,
            group = %config.consumer_group,
            message_topic = %config.message_topic,
            notification_topic = %config.notification_topic,
            task_topic = %config.task_topic,
            "PushServer connected to Kafka"
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
                Ok(record) => {
                    if let Some(payload) = record.payload() {
                        if let Ok(request) = PushMessageRequest::decode(payload) {
                            if let Err(err) =
                                self.command_service.handle_push_message(request).await
                            {
                                error!(?err, "failed to process push message");
                            }
                        } else if let Ok(request) = PushNotificationRequest::decode(payload) {
                            if let Err(err) =
                                self.command_service.handle_push_notification(request).await
                            {
                                error!(?err, "failed to process push notification");
                            }
                        } else {
                            warn!("unknown payload in push server topic");
                        }
                    }
                }
                Err(err) => error!(?err, "error receiving from Kafka"),
            }
        }
    }

    pub fn config(&self) -> &Arc<PushServerConfig> {
        &self.config
    }
}
