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
        
        // 初始化 OpenTelemetry 追踪
        #[cfg(feature = "tracing")]
        {
            let otlp_endpoint = std::env::var("OTLP_ENDPOINT").ok();
            if let Err(e) = flare_im_core::tracing::init_tracing("message-orchestrator", otlp_endpoint.as_deref()) {
                tracing::error!(error = %e, "Failed to initialize OpenTelemetry tracing");
            } else {
                info!("✅ OpenTelemetry tracing initialized");
            }
        }
        
        // 加载应用配置
        let app_config = load_config(Some("./config"));
        let service_config = app_config.message_orchestrator_service();
        
        info!("Parsing server address...");
        let address: SocketAddr = ServiceHelper::parse_server_addr(
            app_config,
            &service_config.runtime,
            "message-orchestrator",
        )
        .context("invalid message orchestrator server address")?;
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
        use flare_proto::message::message_service_server::MessageServiceServer;
        use tonic::transport::Server;

        let handler = context.handler.clone();

        info!(
            address = %address,
            port = %address.port(),
            "Starting Message Orchestrator gRPC service..."
        );

        // 使用 ServiceRuntime 管理服务生命周期
        let address_clone = address;
        let runtime = ServiceRuntime::new("message-orchestrator", address)
            .add_spawn_with_shutdown("message-orchestrator-grpc", move |shutdown_rx| async move {
                Server::builder()
                    .add_service(MessageServiceServer::new(handler))
                    .serve_with_shutdown(address_clone, async move {
                        info!(
                            address = %address_clone,
                            port = %address_clone.port(),
                            "✅ Message Orchestrator gRPC service is listening"
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
                use flare_im_core::service_names::MESSAGE_ORCHESTRATOR;
                match flare_im_core::discovery::register_service_only(MESSAGE_ORCHESTRATOR, addr, None).await {
                    Ok(Some(registry)) => {
                        info!("✅ Service registered: {}", MESSAGE_ORCHESTRATOR);
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

