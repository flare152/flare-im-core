use std::sync::Arc;

use flare_im_core::hooks::{HookDispatcher, HookRegistry};
use flare_server_core::error::{ErrorBuilder, ErrorCode, Result};
use tracing::info;

use crate::application::PushDispatchCommandService;
use crate::infrastructure::cache::redis_online::RedisOnlineStatusRepository;
use crate::infrastructure::config::PushServerConfig;
use crate::infrastructure::messaging::kafka_task_publisher::KafkaPushTaskPublisher;
use crate::interfaces::runtime::consumer::PushKafkaConsumer;

pub struct PushServer {
    consumer: Arc<PushKafkaConsumer>,
}

impl PushServer {
    pub async fn new(server_config: PushServerConfig) -> Result<Self> {
        let server_config = Arc::new(server_config);
        let redis_client = Arc::new(
            redis::Client::open(server_config.redis_url.as_str()).map_err(|err| {
                ErrorBuilder::new(
                    ErrorCode::ServiceUnavailable,
                    "failed to create redis client",
                )
                .details(err.to_string())
                .build_error()
            })?,
        );
        let online_repo = Arc::new(RedisOnlineStatusRepository::new(
            redis_client,
            server_config.online_ttl_seconds,
        ));
        let task_publisher = Arc::new(KafkaPushTaskPublisher::new(server_config.clone())?);
        let hook_registry = HookRegistry::new();
        let hooks = Arc::new(HookDispatcher::new(hook_registry));
        let command_service = Arc::new(PushDispatchCommandService::new(
            server_config.clone(),
            online_repo,
            task_publisher,
            hooks.clone(),
        ));
        let consumer = Arc::new(PushKafkaConsumer::new(server_config, command_service).await?);
        Ok(Self { consumer })
    }

    pub async fn run(&self) -> Result<()> {
        info!(
            bootstrap = %self.consumer.config().kafka_bootstrap,
            group = %self.consumer.config().consumer_group,
            "Push Server started, consuming from Kafka"
        );
        self.consumer.run().await
    }
}
