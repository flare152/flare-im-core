//! 应用启动器 - 负责依赖注入和服务启动
use crate::interface::grpc::server::SignalingRouteServer;
use crate::service::registry::ServiceRegistrar;
use anyhow::Result;
use flare_im_core::{FlareAppConfig, ServiceHelper};
use flare_server_core::ServiceRegistryTrait;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tonic::transport::Server;
use tracing::info;

/// 应用上下文 - 包含所有已初始化的服务
pub struct ApplicationContext {
    pub grpc_server: Arc<SignalingRouteServer>,
    pub service_registry:
        Option<Arc<RwLock<Box<dyn ServiceRegistryTrait>>>>,
    pub service_info: Option<flare_server_core::ServiceInfo>,
}

/// 应用启动器
pub struct ApplicationBootstrap;

impl ApplicationBootstrap {
    /// 运行应用的主入口点
    pub async fn run(config: &'static FlareAppConfig) -> Result<()> {
        // 创建应用上下文
        let context = Self::create_context(config).await?;

        // 获取服务配置并解析服务器地址
        let service_config = config.signaling_route_service();
        let address: SocketAddr = ServiceHelper::parse_server_addr(
            config,
            &service_config.runtime,
            "flare-signaling-route",
        )?;

        // 启动服务器
        Self::start_server(context, address).await
    }

    /// 创建应用上下文
    pub async fn create_context(config: &FlareAppConfig) -> Result<ApplicationContext> {
        let service_config = config.signaling_route_service();
        let runtime_config = config.compose_service_config(
            &service_config.runtime,
            "flare-signaling-route",
        );
        let service_type = "signaling-route".to_string();

        // 注册服务
        let (service_registry, service_info) =
            ServiceRegistrar::register_service(&runtime_config, &service_type).await?;

        // 构建核心服务
        // SignalingRouteServer内部会创建service和repository
        let grpc_server = Arc::new(SignalingRouteServer::new(config, runtime_config.clone()).await?);

        Ok(ApplicationContext {
            grpc_server,
            service_registry,
            service_info,
        })
    }

    /// 启动gRPC服务器
    pub async fn start_server(
        context: ApplicationContext,
        address: SocketAddr,
    ) -> Result<()> {
        info!(%address, "starting signaling route service");

        let server_future = Server::builder()
            .add_service(
                flare_proto::signaling::signaling_service_server::SignalingServiceServer::new(
                    context.grpc_server.as_ref().clone(),
                ),
            )
            .serve(address);

        // 等待服务器启动或接收到停止信号
        let result = tokio::select! {
            res = server_future => {
                if let Err(err) = res {
                    tracing::error!(error = %err, "signaling route service failed");
                }
                Ok(())
            }
            _ = tokio::signal::ctrl_c() => {
                info!("shutdown signal received");
                Ok(())
            }
        };

        // 执行优雅停机
        Self::graceful_shutdown(context).await;

        info!("signaling route service stopped");
        result
    }

    /// 优雅停机处理
    async fn graceful_shutdown(context: ApplicationContext) {
        // 如果有服务注册器，执行服务注销
        if let (Some(registry), Some(service_info)) =
            (&context.service_registry, &context.service_info)
        {
            info!("unregistering service...");
            let mut registry = registry.write().await;
            if let Err(e) = registry.unregister(&service_info.instance_id).await {
                tracing::warn!(error = %e, "failed to unregister service");
            } else {
                info!("service unregistered successfully");
            }
        }

        // 在这里可以添加其他需要优雅停机的资源清理操作
    }
}

