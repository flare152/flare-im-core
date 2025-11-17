mod config;
mod error;
mod handler;
mod infrastructure;
mod interceptor;
mod middleware;
mod repository;
mod router;
mod server;
mod transform;

use anyhow::{Result, anyhow};
use config::GatewayConfig;
use flare_im_core::error::FlareError;
use flare_im_core::{load_config, register_service};
use server::CommunicationCoreGatewayServer;
use std::net::SocketAddr;
use std::sync::Arc;
use tonic::transport::Server;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let app_config = load_config(Some("config"));
    let runtime_config = app_config.base().clone();

    let addr: SocketAddr = format!(
        "{}:{}",
        runtime_config.server.address, runtime_config.server.port
    )
    .parse()?;

    let gateway_config = GatewayConfig::from_app_config(app_config)
        .map_err(|e| anyhow!(format!("Failed to load gateway config: {}", e)))?;

    let _registry: Option<_> = register_service(&runtime_config, "communication-core-gateway")
        .await
        .map_err(|err: FlareError| anyhow!(err.to_string()))?;

    // 创建基础设施客户端
    // 注意：GrpcSignalingClient::new 已经返回 Arc<Self>，不需要再次包装
    let signaling_client = infrastructure::GrpcSignalingClient::new(
        gateway_config.signaling_endpoint.clone(),
    );
    let storage_client = Arc::new(infrastructure::GrpcStorageClient::new(
        gateway_config.message_endpoint.clone(),
    ));
    let push_client = Arc::new(infrastructure::GrpcPushClient::new(
        gateway_config.push_endpoint.clone(),
    ));
    
    // 创建Gateway Router（跨地区路由）
    let gateway_router = infrastructure::GatewayRouterImpl::from_env()
        .map_err(|e| anyhow!(format!("Failed to create gateway router: {}", e)))?;
    
    // 创建AccessGateway Handler（业务系统推送接口）
    // AccessGatewayHandler::new 接受泛型参数，会自动转换为 trait object
    let access_gateway_handler = handler::AccessGatewayHandler::new(
        signaling_client.clone(),
        gateway_router.clone(),
    );

    // 创建数据库连接和仓储（可选，如果配置了数据库URL）
    // 注意：如果数据库连接失败，ServiceRouter将无法创建，但核心通信服务仍可运行
    let db_pool_result = infrastructure::database::create_db_pool_from_env().await;
    let tenant_repository = db_pool_result.as_ref().ok().map(|pool| {
        Arc::new(repository::tenant::TenantRepositoryImpl::new(pool.clone()))
    });
    
    let hook_config_repository = db_pool_result.as_ref().ok().map(|pool| {
        Arc::new(repository::hook::HookConfigRepositoryImpl::new(pool.clone()))
    });
    
    if tenant_repository.is_none() || hook_config_repository.is_none() {
        tracing::warn!("Database connection not available, admin services will be disabled");
    }

    // 创建服务路由器（聚合所有Handler）
    // let router = router::ServiceRouter::new(
    //     signaling_client.clone(),
    //     storage_client.clone(),
    //     push_client.clone(),
    //     Some(tenant_repository),
    //     Some(hook_config_repository),
    // ).await?;

    // 注意：communication_core.proto 已删除
    // 如果需要统一网关功能，可以聚合多个服务的gRPC接口
    // let communication_service = CommunicationCoreGatewayServer::new(gateway_config).await?;

    // 创建拦截器（集成认证、授权、限流中间件）
    let interceptor = interceptor::GatewayInterceptor::from_env()
        .map_err(|e| anyhow!("Failed to create gateway interceptor: {}", e))?;

    info!("Starting Communication Core Gateway on {}", addr);

    // 聚合所有gRPC服务
    let mut server_builder = Server::builder();

    // 注意：communication_core.proto 已删除
    // 业务系统应该使用 AccessGateway 接口推送消息
    // 如果需要统一网关功能，可以聚合多个服务的gRPC接口
    // 1. CommunicationCore服务（核心通信）- 已删除
    // server_builder = server_builder.add_service(
    //     flare_proto::communication_core::communication_core_server::CommunicationCoreServer::new(
    //         communication_service,
    //     ),
    // );

    // TODO: 等待business.proto和admin.proto生成Rust代码后启用以下服务
    // 2. BusinessService服务（业务端）
    // server_builder = server_builder.add_service(
    //     flare_proto::business::message_service_server::MessageServiceServer::new(
    //         router.message_handler.clone(),
    //     ),
    // );
    // server_builder = server_builder.add_service(
    //     flare_proto::business::session_service_server::SessionServiceServer::new(
    //         router.session_handler.clone(),
    //     ),
    // );
    // server_builder = server_builder.add_service(
    //     flare_proto::business::user_service_server::UserServiceServer::new(
    //         router.user_handler.clone(),
    //     ),
    // );
    // server_builder = server_builder.add_service(
    //     flare_proto::business::push_service_server::PushServiceServer::new(
    //         router.push_handler.clone(),
    //     ),
    // );

    // 3. AdminService服务（管理端）
    // server_builder = server_builder.add_service(
    //     flare_proto::admin::tenant_service_server::TenantServiceServer::new(
    //         router.tenant_handler.clone(),
    //     ),
    // );
    // server_builder = server_builder.add_service(
    //     flare_proto::admin::hook_service_server::HookServiceServer::new(
    //         router.hook_handler.clone(),
    //     ),
    // );
    // server_builder = server_builder.add_service(
    //     flare_proto::admin::metrics_service_server::MetricsServiceServer::new(
    //         router.metrics_handler.clone(),
    //     ),
    // );
    // server_builder = server_builder.add_service(
    //     flare_proto::admin::config_service_server::ConfigServiceServer::new(
    //         router.config_handler.clone(),
    //     ),
    // );

    // 注册AccessGateway服务（业务系统推送接口）
    use flare_proto::access_gateway::access_gateway_server::AccessGatewayServer;
    
    // AccessGatewayServer::new 期望的是实现了 AccessGateway trait 的类型，而不是 Arc
    let access_gateway_service = AccessGatewayServer::new(access_gateway_handler);
    
    // 应用拦截器（认证、授权、限流）
    // 注意：拦截器通过Tower middleware实现，需要在Server层面应用
    // 这里暂时直接注册服务，拦截器功能在handler内部通过extract_claims/extract_tenant_context实现
    let server_builder = server_builder.add_service(access_gateway_service);
    
    info!("AccessGateway service registered");
    
    // 启动服务器
    server_builder
        .serve(addr)
        .await
        .map_err(|err| anyhow!(err.to_string()))?;

    Ok(())
}
