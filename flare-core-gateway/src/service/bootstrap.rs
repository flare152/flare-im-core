//! 应用启动器 - 负责依赖注入和服务启动

use std::net::SocketAddr;

use anyhow::{Context, Result};
use tracing::{info, error};

use flare_server_core::runtime::ServiceRuntime;

use super::wire;

/// 应用启动器
pub struct ApplicationBootstrap;

impl ApplicationBootstrap {
    /// 运行应用的主入口点
    pub async fn run() -> Result<()> {
        use flare_im_core::{load_config, ServiceHelper};
        
        // 加载应用配置
        let app_config = load_config(Some("./config"));
        let gateway_config_service = app_config.core_gateway_service();
        let runtime_config = app_config.compose_service_config(
            &gateway_config_service.runtime,
            "flare-core-gateway",
        );
        
        info!("Parsing server address...");
        let address: SocketAddr = ServiceHelper::parse_server_addr(
            app_config,
            &gateway_config_service.runtime,
            "flare-core-gateway",
        )
        .context("invalid core gateway server address")?;
        info!(address = %address, "Server address parsed successfully");
        
        // 使用 Wire 风格的依赖注入构建应用上下文
        let context = wire::initialize(app_config).await?;
        
        info!("ApplicationBootstrap created successfully");
        
        // 运行服务
        Self::run_with_context(context, address).await
    }

    /// 运行服务（带应用上下文）
    async fn run_with_context(
        context: wire::ApplicationContext,
        address: SocketAddr,
    ) -> Result<()> {
        use flare_proto::media::media_service_server::MediaServiceServer;
        use flare_proto::hooks::hook_service_server::HookServiceServer;
        use flare_proto::message::message_service_server::MessageServiceServer;
        use flare_proto::signaling::online::{signaling_service_server::SignalingServiceServer, user_service_server::UserServiceServer};
        use flare_proto::session::session_service_server::SessionServiceServer;
        use tonic::transport::Server;

        let simple_handler = context.simple_handler;
        let lightweight_handler = context.lightweight_handler;

        info!(
            address = %address,
            port = %address.port(),
            "Starting Core Gateway gRPC service..."
        );

        // 使用 ServiceRuntime 管理服务生命周期
        let address_clone = address;
        let runtime = ServiceRuntime::new("core-gateway", address)
            .add_spawn_with_shutdown("core-gateway-grpc", move |shutdown_rx| async move {
                Server::builder()
                    .add_service(MediaServiceServer::new(simple_handler.clone()))
                    .add_service(HookServiceServer::new(simple_handler.clone()))
                    .add_service(MessageServiceServer::new(simple_handler.clone()))
                    .add_service(SignalingServiceServer::new(simple_handler.clone()))
                    .add_service(UserServiceServer::new(simple_handler.clone()))
                    .add_service(SessionServiceServer::new(simple_handler.clone()))
                    .serve_with_shutdown(address_clone, async move {
                        info!(
                            address = %address_clone,
                            port = %address_clone.port(),
                            "✅ Core Gateway gRPC service is listening"
                        );
                        
                        // 同时监听 Ctrl+C 和关闭通道
                        tokio::select! {
                            _ = tokio::signal::ctrl_c() => {
                                tracing::info!("shutdown signal received (Ctrl+C)");
                            }
                            _ = shutdown_rx => {
                                tracing::info!("shutdown signal received (service registration failed)");
                            }
                        }
                    })
                    .await
                    .map_err(|e| format!("gRPC server error: {}", e).into())
            });

        // 运行服务（带服务注册）
        runtime.run_with_registration(|addr| {
            Box::pin(async move {
                // 注册服务（使用常量）
                use flare_im_core::service_names::CORE_GATEWAY;
                match flare_im_core::discovery::register_service_only(CORE_GATEWAY, addr, None).await {
                    Ok(Some(registry)) => {
                        info!("✅ Service registered: {}", CORE_GATEWAY);
                        Ok(Some(registry))
                    }
                    Ok(None) => {
                        info!("Service discovery not configured, skipping registration");
                        Ok(None)
                    }
                    Err(e) => {
                        error!(
                            error = %e,
                            "❌ Service registration failed"
                        );
                        Err(format!("Service registration failed: {}", e).into())
                    }
                }
            })
        }).await
    }
}