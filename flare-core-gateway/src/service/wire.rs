//! Wire 风格的依赖注入模块
//!
//! 类似 Go 的 Wire 框架，提供简单的依赖构建方法

use std::sync::Arc;

use anyhow::{Context, Result};

use crate::config::GatewayConfig;
use crate::interface::grpc::handler::AccessGatewayHandler;
use crate::infrastructure::{GrpcPushClient, GrpcSignalingClient, GrpcStorageClient, RouteServiceClient};

/// 应用上下文 - 包含所有已初始化的服务
pub struct ApplicationContext {
    pub handler: AccessGatewayHandler,
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
    let gateway_config = GatewayConfig::from_app_config(app_config)
        .context("Failed to load gateway config")?;
    
    // 2. 创建服务发现（使用常量，支持环境变量覆盖）
    use flare_im_core::service_names::{SIGNALING_ONLINE, PUSH_SERVER, MESSAGE_ORCHESTRATOR, SIGNALING_ROUTE, ACCESS_GATEWAY, get_service_name};
    
    // 2.1 Signaling 服务发现
    let signaling_service = get_service_name(SIGNALING_ONLINE);
    let signaling_discover = flare_im_core::discovery::create_discover(&signaling_service)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create signaling service discover for {}: {}", signaling_service, e))?;
    
    let signaling_service_client = if let Some(discover) = signaling_discover {
        Some(flare_server_core::discovery::ServiceClient::new(discover))
    } else {
        None
    };
    
    // 2.2 Push 服务发现
    let push_service = get_service_name(PUSH_SERVER);
    let push_discover = flare_im_core::discovery::create_discover(&push_service)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create push service discover for {}: {}", push_service, e))?;
    
    let push_service_client = if let Some(discover) = push_discover {
        Some(flare_server_core::discovery::ServiceClient::new(discover))
    } else {
        None
    };
    
    // 2.3 Route 服务发现（如果启用）
    let route_service_client: Option<RouteServiceClient> = if gateway_config.use_route_service {
        let route_service = get_service_name(SIGNALING_ROUTE);
        let route_discover = flare_im_core::discovery::create_discover(&route_service)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create route service discover for {}: {}", route_service, e))?;
        
        if let Some(discover) = route_discover {
            let service_client = flare_server_core::discovery::ServiceClient::new(discover);
            Some(RouteServiceClient::with_service_client(service_client, "default".to_string()))
        } else {
            tracing::warn!("Route service is enabled but service discovery failed, falling back to direct routing");
            None
        }
    } else {
        None
    };
    
    // 3. 创建基础设施客户端
    let signaling_client = if let Some(service_client) = signaling_service_client {
        GrpcSignalingClient::with_service_client(service_client)
    } else {
        // 降级：使用服务名称（如果没有配置服务发现）
        GrpcSignalingClient::new(signaling_service.clone())
    };
    
    let message_service = get_service_name(MESSAGE_ORCHESTRATOR);
    let _storage_client = Arc::new(GrpcStorageClient::new(message_service.clone()));
    
    let _push_client = if let Some(service_client) = push_service_client {
        GrpcPushClient::with_service_client(service_client)
    } else {
        // 降级：使用服务名称（如果没有配置服务发现）
        GrpcPushClient::new(push_service.clone())
    };
    
    // 4. 创建 Access Gateway 服务发现
    let access_gateway_service = get_service_name(ACCESS_GATEWAY);
    let access_gateway_discover = flare_im_core::discovery::create_discover(&access_gateway_service)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create access gateway service discover for {}: {}", access_gateway_service, e))?;
    
    // 5. 创建 Gateway Router（跨地区路由）
    use flare_im_core::gateway::{GatewayRouter, GatewayRouterConfig};
    let gateway_router_config = GatewayRouterConfig {
        connection_pool_size: 10,
        connection_timeout_ms: 5000,
        connection_idle_timeout_ms: 300_000, // 5分钟空闲超时
        deployment_mode: std::env::var("GATEWAY_DEPLOYMENT_MODE")
            .unwrap_or_else(|_| "single_region".to_string()),
        local_gateway_id: std::env::var("LOCAL_GATEWAY_ID").ok(),
        access_gateway_service: access_gateway_service.clone(),
    };
    
    let gateway_router = if let Some(discover) = access_gateway_discover {
        let service_client = flare_server_core::discovery::ServiceClient::new(discover);
        GatewayRouter::with_service_client(gateway_router_config, service_client)
    } else {
        // 降级：使用服务名称（如果没有配置服务发现）
        GatewayRouter::new(gateway_router_config)
    };
    
    // 6. 构建 AccessGateway Handler
    let handler = AccessGatewayHandler::new(
        signaling_client.clone(),
        gateway_router.clone(),
    );
    
    Ok(ApplicationContext { handler })
}

