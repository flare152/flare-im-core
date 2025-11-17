use std::sync::Arc;
use std::time::Instant;

use anyhow::Result as AnyhowResult;
use flare_im_core::metrics::StorageWriterMetrics;
#[cfg(feature = "tracing")]
use flare_im_core::tracing::{create_span, set_message_id, set_tenant_id, set_error};
use flare_proto::storage::StoreMessageRequest;
use prost::Message as _;
use rdkafka::config::ClientConfig;
use rdkafka::consumer::{CommitMode, Consumer, StreamConsumer};
use rdkafka::message::BorrowedMessage;
use rdkafka::{Message, producer::FutureProducer};
use tracing::{error, info, warn, instrument, Span};
use anyhow::Result;

use crate::application::commands::process_store_message::{
    ProcessStoreMessageCommand, ProcessStoreMessageCommandHandler,
};
use crate::config::StorageWriterConfig;
use crate::domain::message_persistence::PersistenceResult;
use crate::domain::repositories::{
    AckPublisher, ArchiveStoreRepository, HotCacheRepository, MediaAttachmentVerifier,
    MessageIdempotencyRepository, RealtimeStoreRepository, SessionStateRepository,
    UserSyncCursorRepository, WalCleanupRepository,
};
use crate::infrastructure::external::media::MediaAttachmentClient;
use crate::infrastructure::messaging::ack_publisher::KafkaAckPublisher;
use crate::infrastructure::persistence::mongo_store::MongoMessageStore;
use crate::infrastructure::persistence::postgres_store::PostgresMessageStore;
use crate::infrastructure::persistence::redis_cache::RedisHotCacheRepository;
use crate::infrastructure::persistence::redis_idempotency::RedisIdempotencyRepository;
use crate::infrastructure::persistence::redis_wal_cleanup::RedisWalCleanupRepository;
use crate::infrastructure::persistence::session_state::RedisSessionStateRepository;
use crate::infrastructure::persistence::user_cursor::RedisUserCursorRepository;

pub struct StorageWriterConsumer {
    config: Arc<StorageWriterConfig>,
    kafka_consumer: StreamConsumer,
    command_handler: Arc<ProcessStoreMessageCommandHandler>,
    metrics: Arc<StorageWriterMetrics>,
}

impl StorageWriterConsumer {
    pub async fn new(app_config: &flare_im_core::config::FlareAppConfig) -> Result<Self, Box<dyn std::error::Error>> {
        use anyhow::Context;
        let config = Arc::new(
            StorageWriterConfig::from_app_config(app_config)
                .context("Failed to load storage writer configuration")?,
        );

        let consumer: StreamConsumer = ClientConfig::new()
            .set("bootstrap.servers", &config.kafka_bootstrap)
            .set("group.id", &config.kafka_group)
            .set("enable.partition.eof", "false")
            .set("session.timeout.ms", "6000")
            .set("enable.auto.commit", "false")
            .set("fetch.message.max.bytes", "1048576") // 1MB
            .set("max.partition.fetch.bytes", "1048576") // 1MB
            .set("fetch.min.bytes", &config.fetch_min_bytes.to_string())
            // 指定使用 EXTERNAL listener（从宿主机连接）
            .set("security.protocol", "plaintext")
            // 注意: rdkafka 0.38 中 fetch.max.wait.ms 可能不存在，使用默认值
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
            // MongoDB 是可选的，如果连接失败则跳过
            match MongoMessageStore::new(&config).await {
                Ok(Some(store)) => {
                    Some(Arc::new(store) as Arc<dyn RealtimeStoreRepository + Send + Sync>)
                }
                Ok(None) => None,
                Err(err) => {
                    warn!(?err, "Failed to connect to MongoDB, skipping MongoDB storage");
                    None
                }
            };
        let archive_repo: Option<Arc<dyn ArchiveStoreRepository + Send + Sync>> =
            match PostgresMessageStore::new(&config).await? {
                Some(store) => {
                    Some(Arc::new(store) as Arc<dyn ArchiveStoreRepository + Send + Sync>)
                }
                None => None,
            };

        let session_state_repo: Option<Arc<dyn SessionStateRepository + Send + Sync>> =
            match redis_client.as_ref() {
                Some(client) => {
                    Some(Arc::new(RedisSessionStateRepository::new(client.clone())) as Arc<_>)
                }
                None => None,
            };
        let user_cursor_repo: Option<Arc<dyn UserSyncCursorRepository + Send + Sync>> =
            match redis_client.as_ref() {
                Some(client) => {
                    Some(Arc::new(RedisUserCursorRepository::new(client.clone())) as Arc<_>)
                }
                None => None,
            };

        // 初始化指标收集
        let metrics = Arc::new(StorageWriterMetrics::new());
        
        let command_handler = Arc::new(
            ProcessStoreMessageCommandHandler::new(
                idempotency_repo,
                hot_cache_repo,
                realtime_repo,
                archive_repo,
                wal_cleanup_repo,
                ack_publisher,
                media_verifier,
                metrics.clone(),
            )
            .with_session_sync(session_state_repo, user_cursor_repo),
        );

        info!(
            bootstrap = %config.kafka_bootstrap,
            topic = %config.kafka_topic,
            "StorageWriter connected to Kafka"
        );

        Ok(Self {
            config,
            kafka_consumer: consumer,
            command_handler,
            metrics,
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
            // 批量消费消息
            let mut batch = Vec::new();
            
            // 收集一批消息（最多100条，或等待fetch_max_wait_ms）
            let max_records = 100;
            for _ in 0..max_records {
                match tokio::time::timeout(
                    std::time::Duration::from_millis(self.config.fetch_max_wait_ms),
                    self.kafka_consumer.recv(),
                ).await {
                    Ok(Ok(message)) => {
                        batch.push(message);
                    }
                    Ok(Err(e)) => {
                        error!(error = ?e, "Error receiving message");
                        return Err(Box::new(e));
                    }
                    Err(_) => {
                        // 超时，处理已收集的消息
                        break;
                    }
                }
            }
            
            // 批量处理消息
            if !batch.is_empty() {
                if let Err(e) = self.process_batch(batch).await {
                    error!(error = ?e, "Failed to process batch");
                }
            }
        }
    }

    #[instrument(skip(self), fields(batch_size))]
    async fn process_batch(&self, messages: Vec<BorrowedMessage<'_>>) -> Result<(), Box<dyn std::error::Error>> {
        let batch_start = Instant::now();
        let batch_size = messages.len() as f64;
        let span = Span::current();
        span.record("batch_size", batch_size as u64);
        
        // 记录批量大小
        self.metrics.batch_size.observe(batch_size);
        
        let mut requests = Vec::new();
        let mut valid_messages = Vec::new();
        
        // 解析所有消息
        for message in messages {
            let payload = match message.payload() {
                Some(payload) => payload,
                None => {
                    warn!("Kafka message without payload encountered");
                    continue;
                }
            };

            match StoreMessageRequest::decode(payload) {
                Ok(request) => {
                    requests.push(request);
                    valid_messages.push(message);
                }
                Err(err) => {
                    error!(error = ?err, "Failed to decode StoreMessageRequest");
                }
            }
        }
        
        // 并发处理消息
        let mut handles = Vec::new();
        for request in requests {
            let handler = Arc::clone(&self.command_handler);
            handles.push(tokio::spawn(async move {
                handler.handle(ProcessStoreMessageCommand { request }).await
            }));
        }
        
        // 等待所有任务完成
        let mut results = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(Ok(result)) => {
                    results.push(Ok(result));
                }
                Ok(Err(e)) => {
                    results.push(Err(e));
                }
                Err(e) => {
                    error!(error = ?e, "Task join error");
                }
            }
        }
        
        // 根据处理结果决定是否提交offset
        // 如果所有消息都成功，提交所有offset；否则不提交，等待重试
        let all_success = results.iter().all(|r| r.is_ok());
        
        // 记录批量处理耗时
        let batch_duration = batch_start.elapsed();
        self.metrics.messages_persisted_duration_seconds.observe(batch_duration.as_secs_f64());
        
        if all_success {
            // 提交所有消息的offset
            for message in &valid_messages {
                self.commit_message(message);
            }
            
            // 记录批量处理成功
            info!(
                batch_size = valid_messages.len(),
                "Batch messages persisted successfully"
            );
        } else {
            // 有失败的消息，不提交offset，等待重试
            let success_count = results.iter().filter(|r| r.is_ok()).count();
            warn!(
                batch_size = valid_messages.len(),
                success_count = success_count,
                "Some messages failed; keeping offset for retry"
            );
        }
        
        Ok(())
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
