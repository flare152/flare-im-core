//! Wire 风格的依赖注入模块
//!
//! 类似 Go 的 Wire 框架，提供简单的依赖构建方法

use std::sync::Arc;

use anyhow::{Context as AnyhowContext, Result};
use redis::Client;

use crate::application::handlers::{OnlineCommandHandler, OnlineQueryHandler};
use crate::config::OnlineConfig;
use crate::domain::repository::{
    PresenceWatcher, ConversationRepository, SignalPublisher, SubscriptionRepository,
};
use crate::domain::service::{
    OnlineStatusDomainService, SubscriptionDomainService, UserDomainService,
};
use crate::infrastructure::persistence::redis::{
    RedisPresenceWatcher, RedisConversationRepository, RedisSignalPublisher, RedisSubscriptionRepository,
};
use crate::interface::grpc::handler::OnlineHandler;

/// 应用上下文 - 包含所有已初始化的服务
pub struct ApplicationContext {
    pub online_handler: OnlineHandler,
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
    // 1. 加载在线服务配置
    let online_config = Arc::new(
        OnlineConfig::from_app_config(app_config)
            .with_context(|| "Failed to load online service configuration")?,
    );

    // 2. 创建 Redis 客户端
    let redis_client = Arc::new(
        Client::open(online_config.redis_url.as_str()).with_context(|| "Failed to create Redis client")?,
    );

    // 3. 构建仓储
    let conversation_repository: Arc<dyn ConversationRepository> = Arc::new(RedisConversationRepository::new(
        redis_client.clone(),
        online_config.clone(),
    ));

    let subscription_repository: Arc<dyn SubscriptionRepository> = Arc::new(
        RedisSubscriptionRepository::new(redis_client.clone(), online_config.clone()),
    );

    let signal_publisher: Arc<dyn SignalPublisher> = Arc::new(RedisSignalPublisher::new(
        redis_client.clone(),
        online_config.clone(),
    ));

    let presence_watcher: Arc<dyn PresenceWatcher> = Arc::new(RedisPresenceWatcher::new(
        redis_client.clone(),
        online_config.clone(),
    ));

    // 4. 构建领域服务
    let gateway_id = format!(
        "gateway-{}",
        uuid::Uuid::new_v4().to_string()[..8].to_string()
    );
    let online_domain_service = Arc::new(OnlineStatusDomainService::new(
        conversation_repository.clone(),
        gateway_id,
    ));

    let subscription_domain_service = Arc::new(SubscriptionDomainService::new(
        subscription_repository,
        signal_publisher.clone(),
    ));

    let user_domain_service = Arc::new(UserDomainService::new(conversation_repository.clone()));

    // 5. 构建应用层 handlers
    let command_handler = Arc::new(OnlineCommandHandler::new(
        online_domain_service.clone(),
        subscription_domain_service.clone(),
    ));

    // Query handler: 直接使用基础设施层（查询侧不经过领域层）
    let query_handler = Arc::new(OnlineQueryHandler::new(conversation_repository.clone()));

    // 6. 构建 OnlineService Handler（合并了 SignalingService 和 UserService）
    let online_handler = OnlineHandler::new(
        command_handler,
        query_handler,
        user_domain_service,
        presence_watcher,
    );

    Ok(ApplicationContext {
        online_handler,
    })
}
