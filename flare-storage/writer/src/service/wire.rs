//! Wire 风格的依赖注入模块
//!
//! 类似 Go 的 Wire 框架，提供简单的依赖构建方法

use std::sync::Arc;

use anyhow::{Context, Result};
use flare_server_core::kafka::build_kafka_producer;
use rdkafka::producer::FutureProducer;
use tracing::warn;

use crate::application::handlers::MessagePersistenceCommandHandler;
use crate::config::StorageWriterConfig;
use crate::domain::repository::{
    AckPublisher, ArchiveStoreRepository, HotCacheRepository, MediaAttachmentVerifier,
    MessageIdempotencyRepository, RealtimeStoreRepository, SessionStateRepository,
    UserSyncCursorRepository, WalCleanupRepository,
};
use crate::domain::service::MessagePersistenceDomainService;
use crate::infrastructure::external::media::MediaAttachmentClient;
use crate::infrastructure::messaging::ack_publisher::KafkaAckPublisher;
use crate::infrastructure::persistence::mongo_store::MongoMessageStore;
use crate::infrastructure::persistence::postgres_store::PostgresMessageStore;
use crate::infrastructure::persistence::redis_cache::RedisHotCacheRepository;
use crate::infrastructure::persistence::redis_idempotency::RedisIdempotencyRepository;
use crate::infrastructure::persistence::redis_wal_cleanup::RedisWalCleanupRepository;
use crate::infrastructure::persistence::session_state::RedisSessionStateRepository;
use crate::infrastructure::persistence::user_cursor::RedisUserCursorRepository;
use crate::interface::messaging::consumer::StorageWriterConsumer;
use flare_im_core::metrics::StorageWriterMetrics;

/// 应用上下文 - 包含所有已初始化的服务
pub struct ApplicationContext {
    pub consumer: StorageWriterConsumer,
}

/// 构建应用上下文
///
/// 类似 Go Wire 的 Initialize 函数，按照依赖顺序构建所有组件
///
/// # 参数
/// * `app_config` - 应用配置
///
/// # 返回
/// * `ApplicationContext` - 构建好的应用上下文
pub async fn initialize(
    app_config: &flare_im_core::config::FlareAppConfig,
) -> Result<ApplicationContext> {
    // 1. 加载存储写入器配置
    let config = Arc::new(
        StorageWriterConfig::from_app_config(app_config)
            .context("Failed to load storage writer service configuration")?
    );
    
    // 2. 初始化指标收集
    let metrics = Arc::new(StorageWriterMetrics::new());
    
    // 3. 创建 ACK 发布者（可选）
    let ack_publisher = build_ack_publisher(&config)?;
    
    // 4. 创建 Redis 客户端（可选）
    let redis_client = build_redis_client(&config);
    
    // 5. 创建媒体附件验证器（可选）
    let media_verifier = config.media_service_endpoint.as_ref().map(|endpoint| {
        Arc::new(MediaAttachmentClient::new(endpoint.clone()))
            as Arc<dyn MediaAttachmentVerifier + Send + Sync>
    });
    
    // 6. 创建幂等性仓储（可选）
    let idempotency_repo = redis_client.as_ref().map(|client| {
        Arc::new(RedisIdempotencyRepository::new(client.clone(), &config))
            as Arc<dyn MessageIdempotencyRepository + Send + Sync>
    });
    
    // 7. 创建热缓存仓储（可选）
    let hot_cache_repo = redis_client.as_ref().map(|client| {
        Arc::new(RedisHotCacheRepository::new(client.clone(), &config))
            as Arc<dyn HotCacheRepository + Send + Sync>
    });
    
    // 8. 创建 WAL 清理仓储（可选）
    let wal_cleanup_repo = match (&redis_client, &config.wal_hash_key) {
        (Some(client), Some(key)) => Some(Arc::new(RedisWalCleanupRepository::new(
            client.clone(),
            key.clone(),
        )) as Arc<dyn WalCleanupRepository + Send + Sync>),
        _ => None,
    };
    
    // 9. 创建实时存储仓储（MongoDB，可选）
    let realtime_repo: Option<Arc<dyn RealtimeStoreRepository + Send + Sync>> =
        match MongoMessageStore::new(&config).await {
            Ok(Some(store)) => {
                Some(Arc::new(store) as Arc<dyn RealtimeStoreRepository + Send + Sync>)
            }
            Ok(None) => None,
            Err(err) => {
                warn!(error = ?err, "Failed to connect to MongoDB, skipping MongoDB storage");
                None
            }
        };
    
    // 10. 创建归档存储仓储（PostgreSQL，可选）
    let archive_repo: Option<Arc<dyn ArchiveStoreRepository + Send + Sync>> =
        match PostgresMessageStore::new(&config).await {
            Ok(Some(store)) => {
                Some(Arc::new(store) as Arc<dyn ArchiveStoreRepository + Send + Sync>)
            }
            Ok(None) => None,
            Err(err) => {
                warn!(error = ?err, "Failed to connect to PostgreSQL, skipping PostgreSQL storage");
                None
            }
        };
    
    // 11. 创建会话状态仓储（可选）
    let session_state_repo: Option<Arc<dyn SessionStateRepository + Send + Sync>> =
        redis_client.as_ref().map(|client| {
            Arc::new(RedisSessionStateRepository::new(client.clone())) as Arc<_>
        });
    
    // 12. 创建用户游标仓储（可选）
    let user_cursor_repo: Option<Arc<dyn UserSyncCursorRepository + Send + Sync>> =
        redis_client.as_ref().map(|client| {
            Arc::new(RedisUserCursorRepository::new(client.clone())) as Arc<_>
        });
    
    // 13. 初始化指标收集（应用层关注点）
    let metrics = Arc::new(StorageWriterMetrics::new());
    
    // 14. 创建领域服务（不包含指标，符合 DDD 原则）
    let domain_service = Arc::new(MessagePersistenceDomainService::new(
        idempotency_repo,
        hot_cache_repo,
        realtime_repo,
        archive_repo,
        wal_cleanup_repo,
        ack_publisher,
        media_verifier,
        session_state_repo,
        user_cursor_repo,
    ));
    
    // 15. 创建命令处理器（应用层负责指标记录）
    let command_handler = Arc::new(MessagePersistenceCommandHandler::new(
        domain_service,
        metrics.clone(),
    ));
    
    // 16. 构建 Kafka 消费者（使用统一的构建器，与 push-server 完全一致）
    let consumer = StorageWriterConsumer::new(
        config.clone(),
        command_handler.clone(),
        metrics.clone(),
    )
    .await
    .context("Failed to create StorageWriterConsumer")?;
    
    Ok(ApplicationContext { consumer })
}

/// 构建 ACK 发布者
fn build_ack_publisher(
    config: &Arc<StorageWriterConfig>,
) -> Result<Option<Arc<dyn AckPublisher + Send + Sync>>> {
    if let Some(topic) = &config.kafka_ack_topic {
        // 使用统一的 Kafka 生产者构建器（从 flare-server-core）
        let producer = build_kafka_producer(config.as_ref() as &dyn flare_server_core::kafka::KafkaProducerConfig)
            .context("Failed to create Kafka producer for ACK")?;
        
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

/// 构建 Redis 客户端
fn build_redis_client(config: &Arc<StorageWriterConfig>) -> Option<Arc<redis::Client>> {
    config.redis_url.as_ref().and_then(|url| {
        match redis::Client::open(url.as_str()) {
            Ok(client) => Some(Arc::new(client)),
            Err(err) => {
                warn!(error = ?err, "Failed to initialize Redis client; Redis-backed features disabled");
                None
            }
        }
    })
}

