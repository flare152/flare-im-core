//! Wire 风格的依赖注入模块
//!
//! 类似 Go 的 Wire 框架，提供简单的依赖构建方法

use std::sync::Arc;

use anyhow::{Context, Result};

use crate::application::handlers::PushCommandHandler;
use crate::config::PushWorkerConfig;
use crate::domain::repository::{AckPublisher, DlqPublisher, OfflinePushSender, OnlinePushSender};
use crate::domain::service::PushDomainService;
use crate::infrastructure::ack_publisher::{KafkaAckPublisher, NoopAckPublisher};
use crate::infrastructure::dlq_publisher::KafkaDlqPublisher;
use crate::infrastructure::hook::HookExecutor;
use crate::infrastructure::offline::{NoopOfflinePushSender, build_offline_sender};
use crate::infrastructure::online::{NoopOnlinePushSender, build_online_sender};
use crate::interface::consumers::PushWorkerConsumer;
use flare_im_core::gateway::{GatewayRouter, GatewayRouterConfig};
use flare_im_core::hooks::{HookDispatcher, HookRegistry};
use flare_im_core::metrics::PushWorkerMetrics;
use flare_proto::hooks::hook_extension_client::HookExtensionClient;

/// 应用上下文 - 包含所有已初始化的服务
pub struct ApplicationContext {
    pub consumer: Arc<PushWorkerConsumer>,
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
    // 1. 加载 Worker 配置
    let worker_config = Arc::new(PushWorkerConfig::from_app_config(app_config));

    // 2. 初始化服务发现（用于 Gateway Router）
    // 注意：worker 是消费者服务，不需要服务注册，但需要服务发现来查找 Gateway
    // 这里只初始化服务发现，不注册服务
    // 如果配置了 access_gateway_service，则初始化服务发现
    // 由于 worker 不需要注册，我们使用一个虚拟地址
    if worker_config.access_gateway_service.is_some() {
        use std::net::SocketAddr;
        let virtual_address: SocketAddr = "0.0.0.0:0"
            .parse()
            .map_err(|e| anyhow::anyhow!("Failed to parse virtual address: {}", e))?;
        let _registry = flare_im_core::discovery::init_from_config(
            app_config,
            worker_config
                .access_gateway_service
                .as_deref()
                .unwrap_or("push-worker"),
            virtual_address,
            None,
        )
        .await
        .map_err(|e| anyhow::anyhow!("Failed to initialize service discovery: {}", e))?;
    }

    // 3. 构建推送发送器
    let online_sender: Arc<dyn OnlinePushSender> = build_online_sender(&worker_config);
    let offline_sender: Arc<dyn OfflinePushSender> = build_offline_sender(&worker_config);

    // 4. 构建 ACK 发布器
    let ack_publisher: Arc<dyn AckPublisher> = if let Some(ref ack_topic) = worker_config.ack_topic
    {
        KafkaAckPublisher::new(&worker_config.kafka_bootstrap, ack_topic.clone())
            .map_err(|e| anyhow::anyhow!("Failed to create Kafka ACK publisher: {}", e))?
    } else {
        Arc::new(NoopAckPublisher)
    };

    // 5. 构建死信队列发布器
    let dlq_publisher: Arc<dyn DlqPublisher> = KafkaDlqPublisher::new(
        &worker_config.kafka_bootstrap,
        worker_config.dlq_topic.clone(),
    )
    .map_err(|e| anyhow::anyhow!("Failed to create Kafka DLQ publisher: {}", e))?;

    // 6. 构建 Gateway Router（如果配置了 access_gateway_service）
    let gateway_router: Option<Arc<dyn flare_im_core::gateway::GatewayRouterTrait>> =
        if let Some(ref service_name) = worker_config.access_gateway_service {
            let router_config = GatewayRouterConfig {
                access_gateway_service: service_name.clone(),
                ..Default::default()
            };
            let router = GatewayRouter::new(router_config);
            Some(router as Arc<dyn flare_im_core::gateway::GatewayRouterTrait>)
        } else {
            None
        };

    // 7. 构建 Hook Dispatcher
    let hook_registry = HookRegistry::new();
    let hooks = Arc::new(HookDispatcher::new(hook_registry));

    // 8. 构建 Hook Executor
    let hook_client = build_hook_extension_client(&worker_config).await;
    let hook_executor = Arc::new(HookExecutor::new(hook_client));

    // 9. 初始化指标收集
    let metrics = Arc::new(PushWorkerMetrics::new());

    // 10. 构建领域服务
    let domain_service = Arc::new(PushDomainService::new(
        worker_config.clone(),
        online_sender.clone(),
        offline_sender.clone(),
        ack_publisher.clone(),
        dlq_publisher.clone(),
        gateway_router,
        hooks,
        hook_executor,
        metrics.clone(),
    ));

    // 11. 构建命令处理器
    let command_handler = Arc::new(PushCommandHandler::new(domain_service));

    // 12. 构建消费者
    let consumer = Arc::new(
        PushWorkerConsumer::new(
            worker_config.clone(),
            command_handler.clone(),
            metrics.clone(),
        )
        .await
        .context("Failed to create Push Worker consumer")?,
    );

    tracing::info!(
        bootstrap = %worker_config.kafka_bootstrap,
        group = %worker_config.consumer_group,
        "Push Worker initialized"
    );

    Ok(ApplicationContext { consumer })
}

/// 构建 Hook Extension 客户端
async fn build_hook_extension_client(
    config: &Arc<PushWorkerConfig>,
) -> Option<HookExtensionClient<tonic::transport::Channel>> {
    // 从配置中获取 hook_engine_endpoint
    let endpoint = config.hook_engine_endpoint.as_ref()?;

    match tonic::transport::Endpoint::from_shared(endpoint.clone()) {
        Ok(endpoint) => {
            let endpoint_uri = endpoint.uri().to_string();
            match HookExtensionClient::connect(endpoint).await {
                Ok(client) => {
                    tracing::info!(endpoint = %endpoint_uri, "Connected to Hook engine");
                    Some(client)
                }
                Err(err) => {
                    tracing::error!(error = ?err, endpoint = %endpoint_uri, "Failed to connect to Hook Extension service");
                    None
                }
            }
        }
        Err(err) => {
            tracing::error!(error = ?err, endpoint = %endpoint, "Invalid Hook Extension endpoint");
            None
        }
    }
}
