//! Wire 风格的依赖注入模块
//!
//! 类似 Go 的 Wire 框架，提供简单的依赖构建方法

use std::sync::Arc;

use anyhow::{Context, Result};

use crate::application::handlers::{MessageStorageCommandHandler, MessageStorageQueryHandler};
use crate::config::StorageReaderConfig;
use crate::domain::repository::{MessageStorage, VisibilityStorage, MessageStateRepository};
use crate::domain::service::{MessageStorageDomainConfig, MessageStorageDomainService};
use crate::infrastructure::persistence::postgres::PostgresMessageStorage;
use crate::infrastructure::persistence::message_state_repo::PostgresMessageStateRepository;
use crate::interface::grpc::handler::StorageReaderGrpcHandler;

/// 应用上下文 - 包含所有已初始化的服务
pub struct ApplicationContext {
    pub handler: StorageReaderGrpcHandler,
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
    // 1. 加载存储读取器配置
    let config = Arc::new(
        StorageReaderConfig::from_app_config(app_config)
            .context("Failed to load storage reader service configuration")?
    );
    
    // 2. 创建消息存储实例（必须使用 PostgreSQL）
    let storage: Arc<dyn MessageStorage + Send + Sync> = match PostgresMessageStorage::new(&config).await
        .context("Failed to create PostgreSQL storage")? {
        Some(postgres_storage) => {
            tracing::info!("Using PostgreSQL storage");
            Arc::new(postgres_storage)
        }
        None => {
            return Err(anyhow::anyhow!(
                "PostgreSQL URL not configured. Set POSTGRES_URL or STORAGE_POSTGRES_URL, or define postgres profile in config"
            ));
        }
    };
    
    // 3. 创建可见性存储（可选，暂时为 None）
    let visibility_storage: Option<Arc<dyn VisibilityStorage + Send + Sync>> = None;
    
    // 4. 创建消息状态仓储（使用相同的 PostgreSQL 连接池）
    let message_state_repo: Option<Arc<dyn MessageStateRepository + Send + Sync>> = {
        if let Some(url) = &config.postgres_url {
            // 创建新的连接池用于 message_state_repo
            // 注意：这里可以优化为共享连接池，但为了简化，先创建新池
            use sqlx::postgres::PgPoolOptions;
            let pool = PgPoolOptions::new()
                .max_connections(config.postgres_max_connections)
                .min_connections(config.postgres_min_connections)
                .acquire_timeout(std::time::Duration::from_secs(config.postgres_acquire_timeout_seconds))
                .idle_timeout(Some(std::time::Duration::from_secs(config.postgres_idle_timeout_seconds)))
                .max_lifetime(Some(std::time::Duration::from_secs(config.postgres_max_lifetime_seconds)))
                .test_before_acquire(true)
                .connect(url)
                .await
                .context("Failed to create pool for message_state_repo")?;
            Some(Arc::new(PostgresMessageStateRepository::new(Arc::new(pool))))
        } else {
            None
        }
    };
    
    // 5. 构建领域配置
    let domain_config = MessageStorageDomainConfig {
        max_page_size: config.max_page_size,
        default_range_seconds: config.default_range_seconds,
    };
    
    // 6. 构建领域服务
    let domain_service = Arc::new(MessageStorageDomainService::new(
        storage.clone(),
        visibility_storage,
        message_state_repo,
        domain_config,
    ));
    
    // 6. 构建命令处理器
    let command_handler = Arc::new(MessageStorageCommandHandler::new(domain_service.clone()));
    
    // 7. 构建查询处理器（对于基于 seq 的查询，需要使用领域服务）
    let query_handler = Arc::new(MessageStorageQueryHandler::with_domain_service(
        storage,
        domain_service.clone(),
    ));
    
    // 8. 构建 gRPC 处理器
    let grpc_handler = StorageReaderGrpcHandler::new(
        command_handler,
        query_handler,
    ).await?;
    
    Ok(ApplicationContext { handler: grpc_handler })
}
