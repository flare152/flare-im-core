//! 应用启动器 - 负责依赖注入和服务启动

use crate::application::commands::PushExecutionCommandService;
use crate::application::service::PushApplication;
use crate::config::PushWorkerConfig;
use crate::domain::repositories::{OfflinePushSender, OnlinePushSender};
use crate::domain::service::PushService;
use crate::infrastructure::ack_publisher::{AckPublisher, KafkaAckPublisher, NoopAckPublisher};
use crate::infrastructure::dlq_publisher::{DlqPublisher, KafkaDlqPublisher};
use crate::infrastructure::offline::NoopOfflinePushSender;
use crate::infrastructure::online::NoopOnlinePushSender;
use crate::interface::runtime::PushWorkerConsumer;
use crate::service::registry::ServiceRegistrar;
use anyhow::Result;
use flare_im_core::metrics::PushWorkerMetrics;
use flare_im_core::FlareAppConfig;
use flare_server_core::{ServiceInfo, ServiceRegistryTrait};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

/// 应用上下文 - 包含所有已初始化的服务
pub struct ApplicationContext {
    pub consumer: Arc<PushWorkerConsumer>,
    pub service_registry: Option<Arc<RwLock<Box<dyn ServiceRegistryTrait>>>>,
    pub service_info: Option<ServiceInfo>,
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
            if let Err(e) = flare_im_core::tracing::init_tracing("push-worker", otlp_endpoint.as_deref()) {
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
        let service_cfg = config.push_worker_service();
        let runtime_config = config.compose_service_config(&service_cfg.runtime, "flare-push-worker");
        let service_type = runtime_config.service.name.clone();
        let worker_config = Arc::new(PushWorkerConfig::from_app_config(config));

        // 注册服务
        let (service_registry, service_info) =
            ServiceRegistrar::register_service(&runtime_config, &service_type).await?;

        // 构建推送发送器
        let online_sender: Arc<dyn OnlinePushSender> = NoopOnlinePushSender::shared();
        let offline_sender: Arc<dyn OfflinePushSender> = NoopOfflinePushSender::shared();

        // 构建ACK发布器
        let ack_publisher: Arc<dyn AckPublisher> = if let Some(ref ack_topic) = worker_config.ack_topic {
            KafkaAckPublisher::new(&worker_config.kafka_bootstrap, ack_topic.clone())?
        } else {
            Arc::new(NoopAckPublisher)
        };

        // 构建死信队列发布器
        let dlq_publisher: Arc<dyn DlqPublisher> = KafkaDlqPublisher::new(
            &worker_config.kafka_bootstrap,
            worker_config.dlq_topic.clone(),
        )?;

        // 构建领域服务
        let push_service = Arc::new(PushService::new(
            online_sender.clone(),
            offline_sender.clone(),
        ));
        let _application = Arc::new(PushApplication::new(push_service));

        // 初始化指标收集
        let metrics = Arc::new(PushWorkerMetrics::new());

        // 构建应用服务
        let command_service = Arc::new(PushExecutionCommandService::new(
            worker_config.clone(),
            online_sender.clone(),
            offline_sender.clone(),
            ack_publisher.clone(),
            dlq_publisher.clone(),
            metrics.clone(),
        ));

        // 构建消费者
        let consumer = Arc::new(PushWorkerConsumer::new(
            worker_config.clone(),
            command_service.clone(),
            metrics.clone(),
        ).await?);

        info!(
            bootstrap = %consumer.config().kafka_bootstrap,
            group = %consumer.config().consumer_group,
            "Push Worker initialized"
        );

        Ok(ApplicationContext {
            consumer,
            service_registry,
            service_info,
        })
    }

    /// 启动消费者
    pub async fn start_consumer(context: ApplicationContext) -> Result<()> {
        info!("Starting Push Worker");

        // 启动消费者
        let result = context.consumer.run().await.map_err(|e| anyhow::anyhow!("Consumer error: {}", e));

        // 执行优雅停机
        Self::graceful_shutdown(context).await;

        info!("Push Worker stopped");
        result
    }

    /// 优雅停机处理
    async fn graceful_shutdown(context: ApplicationContext) {
        // 如果有服务注册器，执行服务注销
        if let (Some(registry), Some(service_info)) = (&context.service_registry, &context.service_info) {
            info!("unregistering service...");
            let mut registry = registry.write().await;
            if let Err(e) = registry.unregister(&service_info.instance_id).await {
                tracing::warn!(error = %e, "failed to unregister service");
            } else {
                info!("service unregistered successfully");
            }
        }
    }
}

