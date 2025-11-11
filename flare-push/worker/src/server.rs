use std::sync::Arc;

use flare_server_core::error::Result;
use tracing::info;

use crate::application::PushExecutionCommandService;
use crate::domain::repositories::{OfflinePushSender, OnlinePushSender};
use crate::infrastructure::config::PushWorkerConfig;
use crate::infrastructure::offline::noop::NoopOfflinePushSender;
use crate::infrastructure::online::noop::NoopOnlinePushSender;
use crate::interfaces::runtime::PushWorkerConsumer;

pub struct PushWorkerServer {
    consumer: Arc<PushWorkerConsumer>,
}

impl PushWorkerServer {
    pub async fn new(
        config: PushWorkerConfig,
        online_sender: Option<Arc<dyn OnlinePushSender>>,
        offline_sender: Option<Arc<dyn OfflinePushSender>>,
    ) -> Result<Self> {
        let config = Arc::new(config);
        let online_sender = online_sender
            .unwrap_or_else(|| NoopOnlinePushSender::shared() as Arc<dyn OnlinePushSender>);
        let offline_sender = offline_sender
            .unwrap_or_else(|| NoopOfflinePushSender::shared() as Arc<dyn OfflinePushSender>);
        let command_service = Arc::new(PushExecutionCommandService::new(
            online_sender,
            offline_sender,
        ));
        let consumer = Arc::new(PushWorkerConsumer::new(config, command_service).await?);
        Ok(PushWorkerServer { consumer })
    }

    pub async fn run(&self) -> Result<()> {
        info!(
            bootstrap = %self.consumer.config().kafka_bootstrap,
            group = %self.consumer.config().consumer_group,
            "Push Worker started"
        );
        self.consumer.run().await
    }
}
