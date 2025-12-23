//! Wire 风格的依赖注入模块
//!
//! 类似 Go 的 Wire 框架，提供简单的依赖构建方法

use std::sync::Arc;

use anyhow::{Context, Result};

use crate::config::RouteConfig;
use crate::infrastructure::{OnlineServiceClient, forwarder::MessageForwarder};
use crate::application::handlers::{
    DeviceRouteHandler, MessageRoutingHandler,
};
use crate::domain::service::MessageRoutingDomainService;
use crate::domain::repository::RouteRepository;
use crate::infrastructure::persistence::memory::InMemoryRouteRepository;
use crate::interface::grpc::handler::RouteHandler;

/// 应用上下文 - 包含所有已初始化的服务
pub struct ApplicationContext {
    pub handler: RouteHandler,
}

/// 构建应用上下文
///
/// 简化版：只需要创建 Online 服务客户端
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
    let route_config = Arc::new(
        RouteConfig::from_app_config(app_config)
            .context("Failed to load route service configuration")?,
    );

    // 2. 创建 Online 服务客户端
    // 默认连接本地 Online 服务
    let online_endpoint = route_config
        .online_service_endpoint
        .clone()
        .unwrap_or_else(|| "http://127.0.0.1:50061".to_string());

    tracing::info!(endpoint = %online_endpoint, "Connecting to Online service");

    let online_client = Arc::new(
        OnlineServiceClient::new(online_endpoint)
            .await
            .context("Failed to connect to Online service")?,
    );

    // 3. 创建 MessageForwarder（不使用预创建的 ServiceClient，让 MessageForwarder 在需要时创建）
    // MessageForwarder 会根据具体的服务类型（如 message-orchestrator）创建带 svid 过滤的 ServiceClient
    let default_tenant_id = route_config
        .default_tenant_id
        .clone()
        .unwrap_or_else(|| "default".to_string());
    let message_forwarder = Arc::new(
        MessageForwarder::new(default_tenant_id)
    );

    // 4. 创建路由仓储（用于消息路由领域服务）
    let route_repository: Arc<dyn RouteRepository> = Arc::new(InMemoryRouteRepository::new());

    // 5. 创建消息路由领域服务
    let routing_domain_service = Arc::new(
        MessageRoutingDomainService::new(64, route_repository.clone()) // 64 个分片
    );

    // 6. 创建 Application 层处理器
    let device_route_handler = Arc::new(
        DeviceRouteHandler::new(online_client.clone())
    );
    let message_routing_handler = Arc::new(
        MessageRoutingHandler::new(
            routing_domain_service,
            message_forwarder,
        )
    );

    // 7. 构建 gRPC Handler（通过 Application 层）
    let handler = RouteHandler::new(device_route_handler, message_routing_handler);

    Ok(ApplicationContext { handler })
}
