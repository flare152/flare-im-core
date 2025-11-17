//! 应用启动器 - 负责依赖注入和服务启动
use crate::interface::messaging::consumer::StorageWriterConsumer;
use crate::service::registry::ServiceRegistrar;
use anyhow::Result;
use flare_im_core::FlareAppConfig;
use flare_server_core::ServiceRegistryTrait;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

/// 应用上下文 - 包含所有已初始化的服务
pub struct ApplicationContext {
    pub consumer: Arc<StorageWriterConsumer>,
    pub service_registry:
        Option<Arc<RwLock<Box<dyn ServiceRegistryTrait>>>>,
    pub service_info: Option<flare_server_core::ServiceInfo>,
}

/// 应用启动器
pub struct ApplicationBootstrap;

impl ApplicationBootstrap {
    /// 运行应用的主入口点
    pub async fn run(config: &'static FlareAppConfig) -> Result<()> {
        // 初始化 OpenTelemetry 追踪
        #[cfg(feature = "tracing")]
        {
            let otlp_endpoint = std::env::var("OTLP_ENDPOINT").ok();
            if let Err(e) = flare_im_core::tracing::init_tracing("storage-writer", otlp_endpoint.as_deref()) {
                tracing::error!(error = %e, "Failed to initialize OpenTelemetry tracing");
            } else {
                info!("✅ OpenTelemetry tracing initialized");
            }
        }

        // 创建应用上下文
        let context = Self::create_context(config).await?;

        // 启动消费者
        Self::start_consumer(context).await
    }

    /// 创建应用上下文
    pub async fn create_context(config: &FlareAppConfig) -> Result<ApplicationContext> {
        let runtime_config = config.base().clone();
        let service_type = "storage-writer".to_string();

        // 注册服务
        let (service_registry, service_info) =
            ServiceRegistrar::register_service(&runtime_config, &service_type).await?;

        // 构建核心服务
        let consumer = Arc::new(
            StorageWriterConsumer::new(config)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to create consumer: {}", e))?,
        );

        Ok(ApplicationContext {
            consumer,
            service_registry,
            service_info,
        })
    }

    /// 启动Kafka消费者
    pub async fn start_consumer(context: ApplicationContext) -> Result<()> {
        info!("Storage Writer started, consuming from Kafka");

        let consumer_future = context.consumer.consume_messages();

        // 等待消费者启动或接收到停止信号
        let result = tokio::select! {
            res = consumer_future => {
                if let Err(err) = res {
                    tracing::error!(error = %err, "storage writer consumer failed");
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

        info!("storage writer stopped");
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

