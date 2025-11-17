//! 应用启动器 - 负责依赖注入和服务启动

use crate::application::commands::PushDispatchCommandService;
use crate::application::service::PushApplication;
use crate::config::PushServerConfig;
use crate::domain::repositories::{OnlineStatusRepository, PushTaskPublisher};
use crate::domain::service::PushService;
use crate::infrastructure::ack_tracker::AckTracker;
use crate::infrastructure::cache::redis_online::RedisOnlineStatusRepository;
use crate::infrastructure::gateway_router::{GatewayRouterImpl, GatewayRouterTrait};
use crate::infrastructure::message_state::MessageStateTracker;
use crate::infrastructure::messaging::kafka_task_publisher::KafkaPushTaskPublisher;
use crate::infrastructure::signaling::SignalingOnlineClient;
use crate::interface::runtime::consumer::PushKafkaConsumer;
use crate::service::registry::ServiceRegistrar;
use anyhow::Result;
use flare_im_core::metrics::PushServerMetrics;
use flare_im_core::{FlareAppConfig, hooks::{HookDispatcher, HookRegistry}};
use flare_server_core::{ServiceInfo, ServiceRegistryTrait};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

/// 应用上下文 - 包含所有已初始化的服务
pub struct ApplicationContext {
    pub consumer: Arc<PushKafkaConsumer>,
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
            if let Err(e) = flare_im_core::tracing::init_tracing("push-server", otlp_endpoint.as_deref()) {
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
        let service_cfg = config.push_server_service();
        let runtime_config = config.compose_service_config(&service_cfg.runtime, "flare-push-server");
        let service_type = runtime_config.service.name.clone();
        let server_config = Arc::new(PushServerConfig::from_app_config(config));

        // 注册服务
        let (service_registry, service_info) =
            ServiceRegistrar::register_service(&runtime_config, &service_type).await?;

        // 构建 Redis 客户端
        let redis_client = Arc::new(
            redis::Client::open(server_config.redis_url.as_str())
                .map_err(|e| anyhow::anyhow!("Failed to create redis client: {}", e))?,
        );

        // 构建Signaling Online客户端
        let signaling_client = SignalingOnlineClient::new(
            server_config.signaling_endpoint.clone(),
        );

        // 构建在线状态仓库（使用Signaling Online客户端）
        let online_repo = Arc::new(RedisOnlineStatusRepository::new(
            redis_client.clone(),
            server_config.online_ttl_seconds,
            signaling_client.clone(),
            server_config.default_tenant_id.clone(),
        ));

        // 构建任务发布器
        let task_publisher = Arc::new(KafkaPushTaskPublisher::new(server_config.clone())?);

        // 构建领域服务
        let push_service = Arc::new(PushService::new(
            online_repo.clone(),
            task_publisher.clone(),
        ));
        let application = Arc::new(PushApplication::new(push_service));

        // 构建 Gateway Router
        let gateway_router = GatewayRouterImpl::new(server_config.clone())
            as Arc<dyn GatewayRouterTrait>;

        // 构建消息状态跟踪器
        let state_tracker = MessageStateTracker::new(
            server_config.clone(),
            Some(redis_client.clone()),
        );

        // 构建ACK跟踪器
        let ack_tracker = AckTracker::new(server_config.clone());
        
        // 启动ACK监控任务
        let ack_tracker_monitor = Arc::clone(&ack_tracker);
        tokio::spawn(async move {
            if let Err(e) = ack_tracker_monitor.start_monitor().await {
                tracing::warn!(error = %e, "ACK tracker monitor error");
            }
        });

        // 构建 Hook 分发器
        let hook_registry = HookRegistry::new();
        let hooks = Arc::new(HookDispatcher::new(hook_registry));

        // 初始化指标收集
        let metrics = Arc::new(PushServerMetrics::new());

        // 构建命令服务
        let command_service = Arc::new(PushDispatchCommandService::new(
            server_config.clone(),
            online_repo.clone(),
            task_publisher.clone(),
            hooks.clone(),
            gateway_router.clone(),
            state_tracker.clone(),
            ack_tracker.clone(),
            metrics.clone(),
        ));

        // 构建消费者
        let consumer = Arc::new(PushKafkaConsumer::new(
            server_config.clone(),
            command_service.clone(),
            metrics.clone(),
        ).await?);

        info!(
            bootstrap = %consumer.config().kafka_bootstrap,
            group = %consumer.config().consumer_group,
            "Push Server initialized"
        );

        Ok(ApplicationContext {
            consumer,
            service_registry,
            service_info,
        })
    }

    /// 启动消费者
    pub async fn start_consumer(context: ApplicationContext) -> Result<()> {
        info!("Starting Push Server");

        // 启动消费者
        let result = context.consumer.run().await.map_err(|e| anyhow::anyhow!("Consumer error: {}", e));

        // 执行优雅停机
        Self::graceful_shutdown(context).await;

        info!("Push Server stopped");
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

