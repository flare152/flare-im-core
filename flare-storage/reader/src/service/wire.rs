//! Wire 风格的依赖注入模块
//!
//! 类似 Go 的 Wire 框架，提供简单的依赖构建方法

use std::sync::Arc;

use anyhow::{Context, Result};

use crate::application::handlers::{MessageStorageCommandHandler, MessageStorageQueryHandler};
use crate::config::StorageReaderConfig;
use crate::domain::repository::{MessageStorage, VisibilityStorage};
use crate::domain::service::{MessageStorageDomainConfig, MessageStorageDomainService};
use crate::infrastructure::persistence::mongo::MongoMessageStorage;
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
    
    // 2. 创建消息存储实例
    let storage: Arc<dyn MessageStorage + Send + Sync> = if let Some(ref mongo_url) = config.mongo_url {
        tracing::info!(mongo_url = %mongo_url, "Using MongoDB storage");
        Arc::new(
            MongoMessageStorage::new(mongo_url, &config.mongo_database)
                .await
                .context("Failed to create MongoDB storage")?
        )
    } else {
        tracing::info!("MongoDB URL not configured, using default storage");
        Arc::new(MongoMessageStorage::default())
    };
    
    // 3. 创建可见性存储（可选，暂时为 None）
    let visibility_storage: Option<Arc<dyn VisibilityStorage + Send + Sync>> = None;
    
    // 4. 构建领域配置
    let domain_config = MessageStorageDomainConfig {
        max_page_size: config.max_page_size,
        default_range_seconds: config.default_range_seconds,
    };
    
    // 5. 构建领域服务
    let domain_service = Arc::new(MessageStorageDomainService::new(
        storage.clone(),
        visibility_storage,
        domain_config,
    ));
    
    // 6. 构建命令处理器
    let command_handler = Arc::new(MessageStorageCommandHandler::new(domain_service.clone()));
    
    // 7. 构建查询处理器（查询侧直接使用基础设施层）
    let query_handler = Arc::new(MessageStorageQueryHandler::new(storage));
    
    // 8. 构建 gRPC 处理器
    let grpc_handler = StorageReaderGrpcHandler::new(
        command_handler,
        query_handler,
    ).await?;
    
    Ok(ApplicationContext { handler: grpc_handler })
}

