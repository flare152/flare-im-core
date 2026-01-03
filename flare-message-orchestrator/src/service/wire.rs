//! Wire 风格的依赖注入模块
//!
//! 类似 Go 的 Wire 框架，提供简单的依赖构建方法

use std::sync::Arc;

use anyhow::{Context, Result};
use flare_proto::storage::storage_reader_service_client::StorageReaderServiceClient;
use flare_server_core::kafka::build_kafka_producer;

use crate::application::handlers::MessageCommandHandler;
use crate::config::MessageOrchestratorConfig;
use crate::domain::repository::{
    MessageEventPublisherItem, ConversationRepositoryItem, WalRepositoryItem,
};
use crate::domain::service::{MessageDomainService, MessageTemporaryService, SequenceAllocator};
use crate::infrastructure::external::session_client::GrpcConversationClient;
use crate::infrastructure::messaging::kafka_publisher::KafkaMessagePublisher;
use crate::infrastructure::persistence::noop_wal::NoopWalRepository;
use crate::infrastructure::persistence::redis_wal::RedisWalRepository;
use crate::interface::grpc::handler::MessageGrpcHandler;
use flare_im_core::hooks::adapters::DefaultHookFactory;
use flare_im_core::hooks::{HookConfigLoader, HookDispatcher, HookRegistry};
use flare_im_core::metrics::MessageOrchestratorMetrics;
use flare_proto::conversation::conversation_service_client::ConversationServiceClient;

/// 应用上下文 - 包含所有已初始化的服务
pub struct ApplicationContext {
    pub handler: MessageGrpcHandler,
    pub config: Arc<MessageOrchestratorConfig>,
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
    // 1. 加载配置
    let config = Arc::new(MessageOrchestratorConfig::from_app_config(app_config));

    // 2. 创建 Kafka Producer（使用统一的构建器）
    let producer =
        build_kafka_producer(config.as_ref() as &dyn flare_server_core::kafka::KafkaProducerConfig)
            .context("Failed to create Kafka producer")?;

    // 3. 构建消息发布器（new 方法返回 Arc<Self>，包装为 enum）
    let kafka_publisher = KafkaMessagePublisher::new(Arc::new(producer), config.clone());
    let publisher = Arc::new(MessageEventPublisherItem::Kafka(kafka_publisher));

    // 4. 构建 WAL Repository
    let wal_repository =
        build_wal_repository(&config).context("Failed to create WAL repository")?;

    // 5. 构建 Hook Dispatcher
    let hooks = build_hook_dispatcher(&config)
        .await
        .context("Failed to create Hook dispatcher")?;

    // 6. 🔹 构建 SequenceAllocator（核心能力：保证消息顺序）
    let sequence_allocator = build_sequence_allocator(&config)
        .await
        .context("Failed to create SequenceAllocator")?;

    // 7. 初始化指标收集
    let metrics = Arc::new(MessageOrchestratorMetrics::new());

    // 8. 构建 Session 服务客户端（可选）
    let conversation_repository = build_conversation_client(&config).await;

    // 9. 构建领域服务
    let domain_service = Arc::new(MessageDomainService::new(
        Arc::clone(&publisher), // 使用 Arc::clone 避免移动
        wal_repository.clone(), // 先 clone，后续还需要使用
        conversation_repository,
        sequence_allocator,
        config.defaults(),
        hooks,
    ));

    // 10. 构建 Storage Reader 客户端（如果配置了 reader_endpoint）
    let reader_client = build_storage_reader_client(&config).await;

    // 11. 构建查询处理器
    let query_handler = Arc::new(crate::application::handlers::MessageQueryHandler::new(
        domain_service.clone(),
        reader_client.clone().map(|client| Arc::new(client)),
    ));

    // 12. 构建消息操作服务（总是创建，如果没有 reader_client 则使用 Noop MessageRepository）
    use crate::domain::service::message_operation_service::{MessageOperationService, EventPublisher, MessageRepository};
    use crate::domain::model::Message;
    
    let message_repo: Arc<dyn MessageRepository> = if let Some(ref reader_client) = reader_client {
        use crate::infrastructure::persistence::message_repository_adapter::StorageReaderMessageRepository;
        Arc::new(StorageReaderMessageRepository::new(Arc::new(reader_client.clone())))
    } else {
        // 创建 Noop MessageRepository（当 reader_client 不存在时）
        struct NoopMessageRepository;
        #[async_trait::async_trait]
        impl MessageRepository for NoopMessageRepository {
            async fn find_by_id(&self, _message_id: &str) -> Result<Option<Message>> {
                Ok(None) // 总是返回 None，表示消息不存在
            }
            async fn save(&self, _message: &Message) -> Result<()> {
                Ok(()) // Noop，不保存
            }
        }
        Arc::new(NoopMessageRepository)
    };
    
    struct NoopEventPublisher;
    #[async_trait::async_trait]
    impl EventPublisher for NoopEventPublisher {
        async fn publish_recalled(&self, _: &crate::domain::event::MessageRecalledEvent) -> Result<()> { Ok(()) }
        async fn publish_edited(&self, _: &crate::domain::event::MessageEditedEvent) -> Result<()> { Ok(()) }
        async fn publish_deleted(&self, _: &crate::domain::event::MessageDeletedEvent) -> Result<()> { Ok(()) }
        async fn publish_read(&self, _: &crate::domain::event::MessageReadEvent) -> Result<()> { Ok(()) }
        async fn publish_reaction_added(&self, _: &crate::domain::event::MessageReactionAddedEvent) -> Result<()> { Ok(()) }
        async fn publish_reaction_removed(&self, _: &crate::domain::event::MessageReactionRemovedEvent) -> Result<()> { Ok(()) }
        async fn publish_pinned(&self, _: &crate::domain::event::MessagePinnedEvent) -> Result<()> { Ok(()) }
        async fn publish_unpinned(&self, _: &crate::domain::event::MessageUnpinnedEvent) -> Result<()> { Ok(()) }
        async fn publish_favorited(&self, _: &crate::domain::event::MessageFavoritedEvent) -> Result<()> { Ok(()) }
        async fn publish_unfavorited(&self, _: &crate::domain::event::MessageUnfavoritedEvent) -> Result<()> { Ok(()) }
    }
    
    let operation_service = Arc::new(MessageOperationService::new(
        message_repo,
        Arc::new(NoopEventPublisher),
        publisher.clone(),
        Some(wal_repository.clone()), // 注入 WAL Repository 用于 fallback 查询
    ));

    // 13. 构建临时消息处理服务
    let temporary_service = Arc::new(MessageTemporaryService::new(publisher.clone()));

    // 14. 构建命令处理器
    let command_handler = Arc::new(MessageCommandHandler::new(
        domain_service,
        operation_service.clone(),
        Some(temporary_service.clone()),
        metrics,
    ));

    // 15. 构建 gRPC 处理器（只依赖 command_handler 和 query_handler）
    let handler = MessageGrpcHandler::new(
        command_handler,
        query_handler,
    );

    Ok(ApplicationContext {
        handler,
        config,
    })
}

// build_kafka_producer 函数已移除，现在直接使用 flare_server_core::kafka::build_kafka_producer

/// 构建 WAL Repository
fn build_wal_repository(config: &Arc<MessageOrchestratorConfig>) -> Result<Arc<WalRepositoryItem>> {
    if let Some(url) = &config.redis_url {
        let client =
            Arc::new(redis::Client::open(url.as_str()).context("Failed to create Redis client")?);
        Ok(Arc::new(WalRepositoryItem::Redis(Arc::new(
            RedisWalRepository::new(client, config.clone()),
        ))))
    } else {
        Ok(Arc::new(WalRepositoryItem::Noop(Arc::new(
            NoopWalRepository::default(),
        ))))
    }
}

/// 构建 SequenceAllocator（核心能力：保证消息顺序）
///
/// # 设计原理
///
/// 1. 优先使用 Redis 实现（高性能、强一致）
/// 2. 如果未配置 Redis，降级到时间戳模式（性能更高，但不保证严格顺序）
/// 3. 预分配批次大小从配置读取（默认 100）
async fn build_sequence_allocator(
    config: &Arc<MessageOrchestratorConfig>,
) -> Result<Arc<SequenceAllocator>> {
    if let Some(url) = &config.redis_url {
        // Redis 模式（推荐）：强一致性序列号
        let client = Arc::new(
            redis::Client::open(url.as_str())
                .context("Failed to create Redis client for SequenceAllocator")?,
        );

        // 批次大小可以从配置读取（这里默认 100）
        let batch_size = 100;

        tracing::info!(
            redis_url = %url,
            batch_size = batch_size,
            "SequenceAllocator initialized with Redis backend"
        );

        Ok(Arc::new(SequenceAllocator::new(client, batch_size).await?))
    } else {
        // 降级模式：使用虚拟 Redis 客户端（所有操作都返回错误，触发降级到时间戳模式）
        // 这样可以保持统一的接口，不需要特殊处理
        tracing::warn!(
            "Redis not configured, SequenceAllocator will use degraded mode (timestamp-based). \
             This does NOT guarantee strict message ordering!"
        );

        // 创建一个假的 Redis 客户端（连接到无效地址，确保所有操作失败）
        let fake_client = Arc::new(
            redis::Client::open("redis://127.0.0.1:0")
                .context("Failed to create fake Redis client")?,
        );

        Ok(Arc::new(SequenceAllocator::new(fake_client, 100).await?))
    }
}

/// 构建 Hook Dispatcher
async fn build_hook_dispatcher(
    config: &Arc<MessageOrchestratorConfig>,
) -> Result<Arc<HookDispatcher>> {
    let mut hook_loader = HookConfigLoader::new();
    if let Some(path) = &config.hook_config {
        hook_loader = hook_loader.add_candidate(path.clone());
    }
    if let Some(dir) = &config.hook_config_dir {
        hook_loader = hook_loader.add_candidate(dir.clone());
    }
    let hook_config = hook_loader
        .load()
        .map_err(|err| anyhow::anyhow!("Failed to load hook config: {}", err))?;
    let registry = HookRegistry::builder().build();
    let hook_factory = DefaultHookFactory::new()
        .map_err(|err| anyhow::anyhow!("Failed to create hook factory: {}", err))?;
    hook_config
        .install(Arc::clone(&registry), &hook_factory)
        .await
        .map_err(|err| anyhow::anyhow!("Failed to install hooks: {}", err))?;
    Ok(Arc::new(HookDispatcher::new(registry)))
}

/// 构建 Session 服务客户端
async fn build_conversation_client(
    config: &Arc<MessageOrchestratorConfig>,
) -> Option<Arc<ConversationRepositoryItem>> {
    // 使用服务发现创建 session 服务客户端（使用常量，支持环境变量覆盖）
    use flare_im_core::service_names::{CONVERSATION, get_service_name};
    let conversation_service = config
        .conversation_service_type
        .as_deref()
        .map(|s| s.to_string())
        .unwrap_or_else(|| get_service_name(CONVERSATION));

    // 添加超时保护，避免服务发现阻塞整个启动过程
    let discover_result = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        flare_im_core::discovery::create_discover(&conversation_service),
    )
    .await;

    match discover_result {
        Ok(Ok(Some(discover))) => {
            let mut service_client = flare_server_core::discovery::ServiceClient::new(discover);
            // 添加超时保护获取 channel
            match tokio::time::timeout(
                std::time::Duration::from_secs(3),
                service_client.get_channel(),
            )
            .await
            {
                Ok(Ok(channel)) => {
                    tracing::info!(service = %conversation_service, "Connected to Session service via service discovery");
                    Some(Arc::new(ConversationRepositoryItem::Grpc(Arc::new(
                        GrpcConversationClient::new(ConversationServiceClient::new(channel)),
                    ))))
                }
                Ok(Err(err)) => {
                    tracing::warn!(error = %err, service = %conversation_service, "Failed to get Session service channel, session auto-creation disabled");
                    None
                }
                Err(_) => {
                    tracing::warn!(service = %conversation_service, "Timeout getting Session service channel after 3s, session auto-creation disabled");
                    None
                }
            }
        }
        Ok(Ok(None)) => {
            tracing::debug!(service = %conversation_service, "Session service discovery not configured, session auto-creation disabled");
            None
        }
        Ok(Err(err)) => {
            tracing::warn!(error = %err, service = %conversation_service, "Failed to create Session service discover, session auto-creation disabled");
            None
        }
        Err(_) => {
            tracing::warn!(service = %conversation_service, "Timeout creating Session service discover after 3s, session auto-creation disabled");
            None
        }
    }
}

/// 构建 Storage Reader 客户端
async fn build_storage_reader_client(
    config: &Arc<MessageOrchestratorConfig>,
) -> Option<StorageReaderServiceClient<tonic::transport::Channel>> {
    if let Some(endpoint) = &config.reader_endpoint {
        match tonic::transport::Endpoint::from_shared(endpoint.clone()) {
            Ok(endpoint) => match StorageReaderServiceClient::connect(endpoint.clone()).await {
                Ok(client) => {
                    tracing::info!(endpoint = %endpoint.uri(), "Connected to Storage Reader");
                    Some(client)
                }
                Err(err) => {
                    tracing::error!(error = ?err, endpoint = %endpoint.uri(), "Failed to connect to Storage Reader");
                    None
                }
            },
            Err(err) => {
                tracing::error!(error = ?err, endpoint = %endpoint, "Invalid Storage Reader endpoint");
                None
            }
        }
    } else {
        None
    }
}
