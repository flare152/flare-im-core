//! Wire 风格的依赖注入模块
//!
//! 类似 Go 的 Wire 框架，提供简单的依赖构建方法

use std::sync::Arc;

use anyhow::{Context, Result};

use crate::application::handlers::{RouteCommandHandler, RouteQueryHandler};
use crate::config::RouteConfig;
use crate::domain::service::RouteDomainService;
use crate::infrastructure::persistence::memory::InMemoryRouteRepository;
use crate::infrastructure::forwarder::MessageForwarder;
use crate::interface::grpc::handler::RouteHandler;
use flare_server_core::discovery::ServiceClient;

/// 应用上下文 - 包含所有已初始化的服务
pub struct ApplicationContext {
    pub handler: RouteHandler,
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
    // 1. 加载路由配置
    let route_config = Arc::new(
        RouteConfig::from_app_config(app_config)
            .context("Failed to load route service configuration")?
    );
    
    // 2. 创建仓储实现
    let repository: Arc<dyn crate::domain::repository::RouteRepository + Send + Sync> = Arc::new(
        InMemoryRouteRepository::new(),
    );
    
    // 3. 初始化默认路由
    for (svid, endpoint) in &route_config.default_services {
        let route = crate::domain::model::Route::new(svid.clone(), endpoint.clone());
        if let Err(e) = repository.save(route).await {
            tracing::warn!(error = %e, svid = %svid, "Failed to initialize default route");
        }
    }
    
    // 4. 创建领域服务（用于命令侧）
    let domain_service = Arc::new(RouteDomainService::new(repository.clone()));
    
    // 5. 创建 CQRS Handlers
    // Command handler: 使用领域服务（包含业务逻辑）
    let command_handler = Arc::new(RouteCommandHandler::new(domain_service.clone()));
    // Query handler: 使用领域服务（查询侧也通过领域服务）
    let query_handler = Arc::new(RouteQueryHandler::new(domain_service));
    
    // 6. 创建消息转发服务
    let default_tenant_id = "default".to_string();
    
    // 尝试创建服务发现客户端（使用常量）
    use flare_im_core::service_names::{MESSAGE_ORCHESTRATOR, get_service_name};
    let message_orchestrator_service = get_service_name(MESSAGE_ORCHESTRATOR);
    let service_client = match flare_im_core::discovery::create_discover(&message_orchestrator_service).await {
        Ok(Some(discover)) => Some(ServiceClient::new(discover)),
        Ok(None) => {
            tracing::warn!("Service discovery not configured for {}, message forwarding may fail", message_orchestrator_service);
            None
        }
        Err(e) => {
            tracing::warn!(error = %e, service = %message_orchestrator_service, "Failed to create service discover");
            None
        }
    };
    
    let forwarder = if let Some(service_client) = service_client {
        Arc::new(MessageForwarder::with_service_client(service_client, default_tenant_id))
    } else {
        Arc::new(MessageForwarder::new(default_tenant_id))
    };
    
    // 7. 构建 gRPC Handler（传递 repository 给 forwarder）
    let handler = RouteHandler::new(command_handler, query_handler, forwarder, repository.clone());
    
    Ok(ApplicationContext { handler })
}

