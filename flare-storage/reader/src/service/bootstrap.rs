//! 应用启动器 - 负责依赖注入和服务启动
use crate::interface::grpc::server::StorageReaderServer;
use crate::service::registry::ServiceRegistrar;
use anyhow::Result;
use flare_im_core::FlareAppConfig;
use flare_server_core::ServiceRegistryTrait;
use std::sync::Arc;
use tokio::sync::RwLock;
use tonic::transport::Server;
use tracing::info;

/// 应用上下文 - 包含所有已初始化的服务
pub struct ApplicationContext {
    pub grpc_server: Arc<StorageReaderServer>,
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

        // 获取运行时配置
        let runtime_config = config.base().clone();

        // 启动服务器
        Self::start_server(
            context,
            &runtime_config.server.address,
            runtime_config.server.port,
        )
        .await
    }

    /// 创建应用上下文
    pub async fn create_context(config: &FlareAppConfig) -> Result<ApplicationContext> {
        let runtime_config = config.base().clone();
        let service_type = "storage-reader".to_string();

        // 注册服务
        let (service_registry, service_info) =
            ServiceRegistrar::register_service(&runtime_config, &service_type).await?;

        // 构建核心服务
        let grpc_server = Arc::new(StorageReaderServer::new(config, runtime_config.clone()).await?);

        Ok(ApplicationContext {
            grpc_server,
            service_registry,
            service_info,
        })
    }

    /// 启动gRPC服务器
    pub async fn start_server(
        context: ApplicationContext,
        address: &str,
        port: u16,
    ) -> Result<()> {
        let addr = format!("{}:{}", address, port).parse()?;
        info!(%addr, "starting storage reader service");

        let server_future = Server::builder()
            .add_service(
                flare_proto::storage::storage_reader_service_server::StorageReaderServiceServer::new(
                    (*context.grpc_server).clone(),
                ),
            )
            .serve(addr);

        // 等待服务器启动或接收到停止信号
        let result = tokio::select! {
            res = server_future => {
                if let Err(err) = res {
                    tracing::error!(error = %err, "storage reader service failed");
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

        info!("storage reader service stopped");
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

