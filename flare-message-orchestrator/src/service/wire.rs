//! Wire 风格的依赖注入模块
//!
//! 类似 Go 的 Wire 框架，提供简单的依赖构建方法

use std::sync::Arc;

use anyhow::{Context, Result};
use flare_server_core::kafka::build_kafka_producer;
use flare_proto::storage::storage_reader_service_client::StorageReaderServiceClient;

use crate::application::handlers::MessageCommandHandler;
use crate::config::MessageOrchestratorConfig;
use crate::domain::repository::{SessionRepository, WalRepository};
use crate::domain::service::MessageDomainService;
use crate::infrastructure::external::session_client::GrpcSessionClient;
use crate::infrastructure::messaging::kafka_publisher::KafkaMessagePublisher;
use crate::infrastructure::persistence::noop_wal::NoopWalRepository;
use crate::infrastructure::persistence::redis_wal::RedisWalRepository;
use crate::interface::grpc::handler::MessageGrpcHandler;
use flare_im_core::hooks::adapters::DefaultHookFactory;
use flare_im_core::hooks::{HookConfigLoader, HookDispatcher, HookRegistry};
use flare_im_core::metrics::MessageOrchestratorMetrics;

/// 应用上下文 - 包含所有已初始化的服务
pub struct ApplicationContext {
    pub handler: MessageGrpcHandler,
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
    let producer = build_kafka_producer(config.as_ref() as &dyn flare_server_core::kafka::KafkaProducerConfig)
        .context("Failed to create Kafka producer")?;
    
    // 3. 构建消息发布器
    let publisher = Arc::new(KafkaMessagePublisher::new(Arc::new(producer), config.clone()));
    
    // 4. 构建 WAL Repository
    let wal_repository = build_wal_repository(&config)
        .context("Failed to create WAL repository")?;
    
    // 5. 构建 Hook Dispatcher
    let hooks = build_hook_dispatcher(&config).await
        .context("Failed to create Hook dispatcher")?;
    
    // 6. 初始化指标收集
    let metrics = Arc::new(MessageOrchestratorMetrics::new());
    
    // 7. 构建 Session 服务客户端（可选）
    let session_repository = build_session_client(&config).await;
    
    // 8. 构建领域服务
    let domain_service = Arc::new(MessageDomainService::new(
        publisher,
        wal_repository,
        session_repository,
        config.defaults(),
        hooks,
    ));
    
    // 9. 构建命令处理器
    let command_handler = Arc::new(MessageCommandHandler::new(domain_service, metrics));
    
    // 10. 初始化 Storage Reader 客户端（如果配置了 reader_endpoint）
    let reader_client = build_storage_reader_client(&config).await;
    
    // 11. 构建 gRPC 处理器
    let handler = MessageGrpcHandler::new(
        command_handler,
        reader_client,
    );
    
    Ok(ApplicationContext { handler })
}

// build_kafka_producer 函数已移除，现在直接使用 flare_server_core::kafka::build_kafka_producer

/// 构建 WAL Repository
fn build_wal_repository(
    config: &Arc<MessageOrchestratorConfig>,
) -> Result<Arc<dyn WalRepository + Send + Sync>> {
    if let Some(url) = &config.redis_url {
        let client = Arc::new(
            redis::Client::open(url.as_str())
                .context("Failed to create Redis client")?
        );
        Ok(Arc::new(RedisWalRepository::new(client, config.clone())) as Arc<dyn WalRepository + Send + Sync>)
    } else {
        Ok(NoopWalRepository::shared())
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
    let hook_config = hook_loader.load()
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
async fn build_session_client(
    config: &Arc<MessageOrchestratorConfig>,
) -> Option<Arc<dyn SessionRepository + Send + Sync>> {
    // 使用服务发现创建 session 服务客户端（使用常量，支持环境变量覆盖）
    use flare_im_core::service_names::{SESSION, get_service_name};
    let session_service = config.session_service_type.as_deref()
        .map(|s| s.to_string())
        .unwrap_or_else(|| get_service_name(SESSION));
    
    // 添加超时保护，避免服务发现阻塞整个启动过程
    let discover_result = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        flare_im_core::discovery::create_discover(&session_service)
    ).await;
    
    match discover_result {
        Ok(Ok(Some(discover))) => {
            let mut service_client = flare_server_core::discovery::ServiceClient::new(discover);
            // 添加超时保护获取 channel
            match tokio::time::timeout(
                std::time::Duration::from_secs(3),
                service_client.get_channel()
            ).await {
                Ok(Ok(channel)) => {
                    tracing::info!(service = %session_service, "Connected to Session service via service discovery");
                    Some(Arc::new(GrpcSessionClient::new(channel)) as Arc<dyn SessionRepository + Send + Sync>)
                }
                Ok(Err(err)) => {
                    tracing::warn!(error = %err, service = %session_service, "Failed to get Session service channel, session auto-creation disabled");
                    None
                }
                Err(_) => {
                    tracing::warn!(service = %session_service, "Timeout getting Session service channel after 3s, session auto-creation disabled");
                    None
                }
            }
        }
        Ok(Ok(None)) => {
            tracing::debug!(service = %session_service, "Session service discovery not configured, session auto-creation disabled");
            None
        }
        Ok(Err(err)) => {
            tracing::warn!(error = %err, service = %session_service, "Failed to create Session service discover, session auto-creation disabled");
            None
        }
        Err(_) => {
            tracing::warn!(service = %session_service, "Timeout creating Session service discover after 3s, session auto-creation disabled");
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
            Ok(endpoint) => {
                match StorageReaderServiceClient::connect(endpoint.clone()).await {
                    Ok(client) => {
                        tracing::info!(endpoint = %endpoint.uri(), "Connected to Storage Reader");
                        Some(client)
                    }
                    Err(err) => {
                        tracing::error!(error = ?err, endpoint = %endpoint.uri(), "Failed to connect to Storage Reader");
                        None
                    }
                }
            }
            Err(err) => {
                tracing::error!(error = ?err, endpoint = %endpoint, "Invalid Storage Reader endpoint");
                None
            }
        }
    } else {
        None
    }
}

