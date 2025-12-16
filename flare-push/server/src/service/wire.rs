//! Wire 风格的依赖注入模块
//!
//! 类似 Go 的 Wire 框架，提供简单的依赖构建方法

use std::sync::Arc;

use anyhow::{Context, Result};

use crate::application::handlers::PushCommandHandler;
use crate::config::PushServerConfig;
use crate::domain::service::PushDomainService;
use crate::infrastructure::ack_tracker::AckTracker;
use crate::infrastructure::cache::online_status_cache::CachedOnlineStatusRepository;
use crate::infrastructure::cache::redis_online::OnlineStatusRepositoryImpl;
use crate::infrastructure::message_state::MessageStateTracker;
use crate::infrastructure::mq::kafka_task_publisher::KafkaPushTaskPublisher;
use crate::infrastructure::session_client::SessionServiceClient;
use crate::infrastructure::signaling::SignalingOnlineClient;
use crate::interface::consumers::{AckKafkaConsumer, PushKafkaConsumer};
use deadpool_redis;
use flare_im_core::ack::{AckModule, AckServiceConfig};
use flare_im_core::gateway::{GatewayRouter, GatewayRouterConfig, GatewayRouterTrait};
use flare_im_core::hooks::{HookDispatcher, HookRegistry};
use flare_im_core::metrics::PushServerMetrics;
use flare_im_core::service_names::{ACCESS_GATEWAY, SESSION, SIGNALING_ONLINE, get_service_name};

/// 应用上下文 - 包含所有已初始化的服务
///
/// 注意：Push Server 是纯消费者，不提供 gRPC 服务
/// ACK 通过 Push Proxy → Kafka → Push Server 的方式传递
pub struct ApplicationContext {
    pub consumer: Arc<PushKafkaConsumer>,
    pub ack_consumer: Arc<AckKafkaConsumer>,
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
    // 1. 加载服务器配置
    let server_config = Arc::new(PushServerConfig::from_app_config(app_config));

    // 2. 创建 Signaling 服务发现（从 service_names 获取，支持环境变量覆盖）
    let signaling_service = get_service_name(SIGNALING_ONLINE);
    let signaling_discover = flare_im_core::discovery::create_discover(&signaling_service)
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to create signaling service discover for {}: {}",
                signaling_service,
                e
            )
        })?;

    let signaling_service_client = if let Some(discover) = signaling_discover {
        Some(flare_server_core::discovery::ServiceClient::new(discover))
    } else {
        None
    };

    // 3. 构建 Signaling Online 客户端（服务发现必需）
    let signaling_client = if let Some(service_client) = signaling_service_client {
        SignalingOnlineClient::with_service_client(service_client)
    } else {
        return Err(anyhow::anyhow!(
            "Service discovery is required for Signaling Online service"
        ));
    };

    // 3.1 创建 Session 服务发现（从 service_names 获取，支持环境变量覆盖）
    let session_service = get_service_name(SESSION);
    let session_discover = flare_im_core::discovery::create_discover(&session_service)
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to create session service discover for {}: {}",
                session_service,
                e
            )
        })?;

    let session_service_client = if let Some(discover) = session_discover {
        Some(flare_server_core::discovery::ServiceClient::new(discover))
    } else {
        None
    };

    // 3.2 构建 Session 服务客户端（可选，用于查询会话参与者）
    let session_client = if let Some(service_client) = session_service_client {
        Some(SessionServiceClient::with_service_client(service_client))
    } else {
        tracing::warn!(
            "Session service discovery not configured, get_all_online_users_for_session will not work"
        );
        None
    };

    // 4. 构建在线状态仓库（带5秒TTL本地缓存）
    let inner_online_repo = if let Some(session_client) = session_client {
        Arc::new(OnlineStatusRepositoryImpl::with_session_client(
            signaling_client.clone(),
            session_client,
            server_config.default_tenant_id.clone(),
        ))
    } else {
        Arc::new(OnlineStatusRepositoryImpl::new(
            signaling_client.clone(),
            server_config.default_tenant_id.clone(),
        ))
    };
    let online_repo = Arc::new(CachedOnlineStatusRepository::new(
        inner_online_repo,
        5, // 5秒TTL
    ));

    // 5. 构建任务发布器
    let task_publisher = Arc::new(
        KafkaPushTaskPublisher::new(server_config.clone())
            .context("Failed to create Kafka push task publisher")?,
    );

    // 6. 创建 Access Gateway 服务发现（从 service_names 获取，支持环境变量覆盖）
    let access_gateway_service = get_service_name(ACCESS_GATEWAY);
    let access_gateway_discover =
        flare_im_core::discovery::create_discover(&access_gateway_service)
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to create access gateway service discover for {}: {}",
                    access_gateway_service,
                    e
                )
            })?;

    // 7. 构建 Gateway Router
    let gateway_router_config = GatewayRouterConfig {
        connection_pool_size: server_config.gateway_router_connection_pool_size,
        connection_timeout_ms: server_config.gateway_router_connection_timeout_ms,
        connection_idle_timeout_ms: server_config.gateway_router_connection_idle_timeout_ms,
        deployment_mode: server_config.gateway_deployment_mode.clone(),
        local_gateway_id: server_config.local_gateway_id.clone(),
        access_gateway_service: access_gateway_service.clone(),
    };

    // Gateway Router（服务发现必需）
    // 注意：ServiceClient::new 消费了 discover，但我们需要同时保存 discover 来根据 gateway_id 过滤实例
    // 解决方案：创建两个独立的 discover 实例（一个用于 ServiceClient，一个用于 Gateway Router）
    // 由于 ServiceDiscover 不支持 Clone，我们需要创建两个独立的 discover 实例
    let gateway_router = if access_gateway_discover.is_some() {
        // 创建两个独立的 discover 实例（一个用于 ServiceClient，一个用于 Gateway Router）
        // 这样 Gateway Router 可以根据 gateway_id 过滤特定的实例
        let discover_for_client =
            flare_im_core::discovery::create_discover(&access_gateway_service)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to create discover for ServiceClient: {}", e))?
                .ok_or_else(|| {
                    anyhow::anyhow!("Service discovery not configured for Access Gateway service")
                })?;
        let discover_for_router =
            flare_im_core::discovery::create_discover(&access_gateway_service)
                .await
                .map_err(|e| {
                    anyhow::anyhow!("Failed to create discover for Gateway Router: {}", e)
                })?
                .ok_or_else(|| {
                    anyhow::anyhow!("Service discovery not configured for Access Gateway service")
                })?;

        let service_client = flare_server_core::discovery::ServiceClient::new(discover_for_client);
        GatewayRouter::with_service_client_and_discover(
            gateway_router_config,
            service_client,
            discover_for_router,
        ) as Arc<dyn GatewayRouterTrait>
    } else {
        return Err(anyhow::anyhow!(
            "Service discovery is required for Access Gateway service"
        ));
    };

    // 8. 构建 Redis 客户端（用于消息状态跟踪）
    let redis_client = Arc::new(
        redis::Client::open(server_config.redis_url.as_str())
            .context("Failed to create Redis client")?,
    );

    // 9. 构建消息状态跟踪器
    let state_tracker = MessageStateTracker::new(server_config.clone(), Some(redis_client.clone()));

    // 10. 创建 Redis 连接池（用于 ACK 重试计数）
    let redis_pool = deadpool_redis::Config::from_url(server_config.redis_url.clone())
        .create_pool(Some(deadpool_redis::Runtime::Tokio1))
        .context("Failed to create Redis pool")?;

    // 11. 初始化统一的 ACK 模块（从业务模块配置中读取）
    let ack_config = AckServiceConfig {
        redis_url: server_config.redis_url.clone(),
        redis_ttl: server_config.ack_redis_ttl,
        cache_capacity: server_config.ack_cache_capacity,
        batch_interval_ms: server_config.ack_batch_interval_ms,
        batch_size: server_config.ack_batch_size,
        // 使用默认的业务场景配置（可以根据需要从配置文件读取）
        ..AckServiceConfig::default()
    };
    let ack_module = Arc::new(
        AckModule::new(ack_config)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to initialize ACK module: {}", e))?,
    );

    // 12. 构建 ACK 跟踪器（使用统一的 AckManager）
    let ack_tracker = AckTracker::new(ack_module.clone(), server_config.clone())
        .with_task_publisher(task_publisher.clone())
        .with_redis_pool(redis_pool.clone());

    // 12. 启动 ACK 监控任务
    let ack_tracker_monitor = Arc::clone(&ack_tracker);
    tokio::spawn(async move {
        ack_tracker_monitor.start_monitor();
    });

    // 12. 构建 Hook 分发器
    let hook_registry = HookRegistry::new();
    let hooks = Arc::new(HookDispatcher::new(hook_registry));

    // 13. 初始化指标收集
    let metrics = Arc::new(PushServerMetrics::new());

    // 14. 构建领域服务
    let domain_service = Arc::new(PushDomainService::new(
        server_config.clone(),
        online_repo.clone(),
        task_publisher.clone(),
        hooks.clone(),
        gateway_router.clone(),
        state_tracker.clone(),
        ack_tracker,
        metrics.clone(),
    ));

    // 15. 构建命令处理器
    let command_handler = Arc::new(PushCommandHandler::new(domain_service.clone()));

    // 16. 构建推送消息消费者
    let consumer = Arc::new(
        PushKafkaConsumer::new(
            server_config.clone(),
            command_handler.clone(),
            metrics.clone(),
        )
        .await
        .context("Failed to create Push Kafka consumer")?,
    );

    // 17. 构建 ACK 消费者
    let ack_consumer = Arc::new(
        AckKafkaConsumer::new(server_config.clone(), domain_service.clone())
            .await
            .context("Failed to create ACK Kafka consumer")?,
    );

    tracing::info!(
        bootstrap = %server_config.kafka_bootstrap,
        group = %server_config.consumer_group,
        task_topic = %server_config.task_topic,
        ack_topic = %server_config.ack_topic,
        "Push Server initialized (pure consumer, no gRPC service)"
    );

    Ok(ApplicationContext {
        consumer,
        ack_consumer,
    })
}
