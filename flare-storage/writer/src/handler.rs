use std::sync::Arc;

use anyhow::Result as AnyhowResult;
use flare_proto::storage::StoreMessageRequest;
use prost::Message as _;
use rdkafka::Message;
use rdkafka::config::ClientConfig;
use rdkafka::consumer::{CommitMode, Consumer, StreamConsumer};
use rdkafka::message::BorrowedMessage;
use rdkafka::producer::FutureProducer;
use tracing::{error, info, warn};

use crate::application::commands::process_store_message::{
    ProcessStoreMessageCommand, ProcessStoreMessageCommandHandler,
};
use crate::domain::message_persistence::PersistenceResult;
use crate::domain::repositories::{
    AckPublisher, ArchiveStoreRepository, HotCacheRepository, MediaAttachmentVerifier,
    MessageIdempotencyRepository, RealtimeStoreRepository, WalCleanupRepository,
};
use crate::infrastructure::config::StorageWriterConfig;
use crate::infrastructure::external::media::MediaAttachmentClient;
use crate::infrastructure::messaging::ack_publisher::KafkaAckPublisher;
use crate::infrastructure::persistence::mongo_store::MongoMessageStore;
use crate::infrastructure::persistence::postgres_store::PostgresMessageStore;
use crate::infrastructure::persistence::redis_cache::RedisHotCacheRepository;
use crate::infrastructure::persistence::redis_idempotency::RedisIdempotencyRepository;
use crate::infrastructure::persistence::redis_wal_cleanup::RedisWalCleanupRepository;

pub struct StorageWriterHandler {
    kafka_consumer: StreamConsumer,
    command_handler: Arc<ProcessStoreMessageCommandHandler>,
}

impl StorageWriterHandler {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let config = Arc::new(StorageWriterConfig::from_env());

        let consumer: StreamConsumer = ClientConfig::new()
            .set("bootstrap.servers", &config.kafka_bootstrap)
            .set("group.id", &config.kafka_group)
            .set("enable.partition.eof", "false")
            .set("session.timeout.ms", "6000")
            .set("enable.auto.commit", "false")
            .create()?;
        consumer.subscribe(&[&config.kafka_topic])?;

        let ack_publisher = Self::build_ack_publisher(&config)?;
        let redis_client = Self::build_redis_client(&config);
        let media_verifier = config.media_service_endpoint.as_ref().map(|endpoint| {
            Arc::new(MediaAttachmentClient::new(endpoint.clone()))
                as Arc<dyn MediaAttachmentVerifier + Send + Sync>
        });

        let idempotency_repo = redis_client.as_ref().map(|client| {
            Arc::new(RedisIdempotencyRepository::new(client.clone(), &config))
                as Arc<dyn MessageIdempotencyRepository + Send + Sync>
        });
        let hot_cache_repo = redis_client.as_ref().map(|client| {
            Arc::new(RedisHotCacheRepository::new(client.clone(), &config))
                as Arc<dyn HotCacheRepository + Send + Sync>
        });
        let wal_cleanup_repo = match (&redis_client, &config.wal_hash_key) {
            (Some(client), Some(key)) => Some(Arc::new(RedisWalCleanupRepository::new(
                client.clone(),
                key.clone(),
            ))
                as Arc<dyn WalCleanupRepository + Send + Sync>),
            _ => None,
        };

        let realtime_repo: Option<Arc<dyn RealtimeStoreRepository + Send + Sync>> =
            match MongoMessageStore::new(&config).await? {
                Some(store) => {
                    Some(Arc::new(store) as Arc<dyn RealtimeStoreRepository + Send + Sync>)
                }
                None => None,
            };
        let archive_repo: Option<Arc<dyn ArchiveStoreRepository + Send + Sync>> =
            match PostgresMessageStore::new(&config).await? {
                Some(store) => {
                    Some(Arc::new(store) as Arc<dyn ArchiveStoreRepository + Send + Sync>)
                }
                None => None,
            };

        let command_handler = Arc::new(ProcessStoreMessageCommandHandler::new(
            idempotency_repo,
            hot_cache_repo,
            realtime_repo,
            archive_repo,
            wal_cleanup_repo,
            ack_publisher,
            media_verifier,
        ));

        info!(
            bootstrap = %config.kafka_bootstrap,
            topic = %config.kafka_topic,
            "StorageWriter connected to Kafka"
        );

        Ok(Self {
            kafka_consumer: consumer,
            command_handler,
        })
    }

    fn build_ack_publisher(
        config: &Arc<StorageWriterConfig>,
    ) -> Result<Option<Arc<dyn AckPublisher + Send + Sync>>, Box<dyn std::error::Error>> {
        if let Some(topic) = &config.kafka_ack_topic {
            let producer: FutureProducer = ClientConfig::new()
                .set("bootstrap.servers", &config.kafka_bootstrap)
                .set("message.timeout.ms", &config.kafka_timeout_ms.to_string())
                .create()?;
            let producer = Arc::new(producer);
            let publisher: Arc<dyn AckPublisher + Send + Sync> = Arc::new(KafkaAckPublisher::new(
                producer,
                config.clone(),
                topic.clone(),
            ));
            Ok(Some(publisher))
        } else {
            Ok(None)
        }
    }

    fn build_redis_client(config: &Arc<StorageWriterConfig>) -> Option<Arc<redis::Client>> {
        config.redis_url.as_ref().and_then(|url| match redis::Client::open(url.as_str()) {
            Ok(client) => Some(Arc::new(client)),
            Err(err) => {
                warn!(error = ?err, "Failed to initialise Redis client; Redis-backed features disabled");
                None
            }
        })
    }

    pub async fn consume_messages(&self) -> Result<(), Box<dyn std::error::Error>> {
        loop {
            match self.kafka_consumer.recv().await {
                Ok(message) => {
                    let payload = match message.payload() {
                        Some(payload) => payload,
                        None => {
                            warn!("Kafka message without payload encountered");
                            if let Err(err) = self
                                .kafka_consumer
                                .commit_message(&message, CommitMode::Async)
                            {
                                warn!(error = ?err, "Failed to commit empty Kafka message");
                            }
                            continue;
                        }
                    };

                    match StoreMessageRequest::decode(payload) {
                        Ok(request) => match self.process_store_message(request).await {
                            Ok(result) => {
                                self.commit_message(&message);
                                info!(
                                    session_id = %result.session_id,
                                    message_id = %result.message_id,
                                    deduplicated = result.deduplicated,
                                    "Message persisted"
                                );
                            }
                            Err(err) => {
                                error!(error = ?err, "Failed to persist message; keeping offset for retry");
                            }
                        },
                        Err(err) => {
                            error!(error = ?err, "Failed to decode StoreMessageRequest");
                        }
                    }
                }
                Err(err) => {
                    error!(error = ?err, "Error while receiving from Kafka");
                }
            }
        }
    }

    async fn process_store_message(
        &self,
        request: StoreMessageRequest,
    ) -> AnyhowResult<PersistenceResult> {
        self.command_handler
            .handle(ProcessStoreMessageCommand { request })
            .await
    }

    fn commit_message(&self, message: &BorrowedMessage<'_>) {
        if let Err(err) = self
            .kafka_consumer
            .commit_message(message, CommitMode::Async)
        {
            warn!(error = ?err, "Failed to commit Kafka message");
        }
    }
}
