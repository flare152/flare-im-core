//! Wire 风格的依赖注入模块
//!
//! 类似 Go 的 Wire 框架，提供简单的依赖构建方法

use std::sync::Arc;

use anyhow::{Context, Result};

use crate::application::handlers::{SessionCommandHandler, SessionQueryHandler};
use crate::config::SessionConfig;
use crate::domain::model::SessionDomainConfig;
use crate::domain::repository::MessageProvider;
use crate::domain::service::SessionDomainService;
use crate::infrastructure::persistence::PostgresSessionRepository;
use crate::infrastructure::persistence::redis_presence::RedisPresenceRepository;
use crate::infrastructure::persistence::redis_repository::RedisSessionRepository;
use crate::infrastructure::transport::storage_reader::StorageReaderMessageProvider;
use crate::interface::grpc::handler::SessionGrpcHandler;

/// 应用上下文 - 包含所有已初始化的服务
pub struct ApplicationContext {
    pub handler: SessionGrpcHandler,
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
    // 1. 加载会话配置
    let session_config = Arc::new(
        SessionConfig::from_app_config(app_config)
            .context("Failed to load session service configuration")?,
    );

    // 2. 创建 Redis 客户端
    let redis_client = Arc::new(redis::Client::open(session_config.redis_url.clone())?);

    // 3. 创建 PostgreSQL 连接池（可选）
    let postgres_pool = if let Some(ref postgres_url) = session_config.postgres_url {
        Some(Arc::new(
            sqlx::PgPool::connect(postgres_url)
                .await
                .context("Failed to connect to PostgreSQL")?,
        ))
    } else {
        None
    };

    // 4. 创建会话仓储
    let session_repo: Arc<dyn crate::domain::repository::SessionRepository> =
        if let Some(ref pool) = postgres_pool {
            let repo = PostgresSessionRepository::new(pool.clone(), session_config.clone());
            // 初始化数据库表结构
            repo.init_schema()
                .await
                .context("Failed to initialize PostgreSQL session schema")?;
            Arc::new(repo)
        } else {
            Arc::new(RedisSessionRepository::new(
                redis_client.clone(),
                session_config.clone(),
            ))
        };

    // 5. 创建在线状态仓储
    let presence_repo = Arc::new(RedisPresenceRepository::new(
        redis_client.clone(),
        session_config.clone(),
    )) as Arc<dyn crate::domain::repository::PresenceRepository>;

    // 6. 创建消息提供者（可选，使用常量）
    // 注意：服务名已统一在 service_names.rs 中定义，不再从配置读取
    let message_provider: Option<Arc<dyn MessageProvider + Send + Sync>> = {
        use flare_im_core::service_names::{STORAGE_READER, get_service_name};
        let storage_reader_service = get_service_name(STORAGE_READER);

        // 创建 Storage Reader 服务发现
        let storage_discover = flare_im_core::discovery::create_discover(&storage_reader_service)
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to create storage reader service discover for {}: {}",
                    storage_reader_service,
                    e
                )
            })?;

        let provider = if let Some(discover) = storage_discover {
            let service_client = flare_server_core::discovery::ServiceClient::new(discover);
            StorageReaderMessageProvider::with_service_client(service_client)
        } else {
            // Fallback: construct provider with service name; provider will try env direct connect
            tracing::warn!("Storage Reader service discovery not configured, using env fallback");
            StorageReaderMessageProvider::new(storage_reader_service)
        };

        Some(Arc::new(provider) as Arc<dyn MessageProvider + Send + Sync>)
    };

    // 7. 构建领域配置
    let domain_config = SessionDomainConfig::new(session_config.recent_message_limit);

    // 8. 转换 message_provider 类型
    let message_provider_for_domain: Option<Arc<dyn MessageProvider>> = message_provider
        .clone()
        .map(|p| p as Arc<dyn MessageProvider>);

    // 9. 构建领域服务
    let domain_service = Arc::new(SessionDomainService::new(
        session_repo.clone(),
        presence_repo,
        message_provider_for_domain,
        domain_config,
    ));

    // 10. 构建命令处理器
    let command_handler = Arc::new(SessionCommandHandler::new(domain_service.clone()));

    // 11. 构建查询处理器
    let query_handler = Arc::new(SessionQueryHandler::new(
        session_repo,
        message_provider,
        domain_service,
    ));

    // 12. 构建 gRPC 处理器
    let grpc_handler = SessionGrpcHandler::new(command_handler, query_handler, None);

    Ok(ApplicationContext {
        handler: grpc_handler,
    })
}
