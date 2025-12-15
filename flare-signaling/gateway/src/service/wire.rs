//! Wire 风格的依赖注入模块
//!
//! 类似 Go 的 Wire 框架，提供简单的依赖构建方法

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use uuid::Uuid;

use crate::application::handlers::{ConnectionQueryService, PushMessageService, SessionCommandService, SessionQueryService};
use crate::application::services::{ConnectionApplicationService, MessageApplicationService};
use crate::config::AccessGatewayConfig;
use crate::domain::repository::{ConnectionQuery, SignalingGateway};
use crate::domain::service::{GatewayService, PushDomainService, SessionDomainService};
use crate::infrastructure::auth::TokenAuthenticator;
use crate::infrastructure::connection_query::ManagerConnectionQuery;
use crate::infrastructure::signaling::grpc::GrpcSignalingGateway;
use crate::infrastructure::{AckPublisher, GrpcAckPublisher, NoopAckPublisher};
use crate::infrastructure::messaging::ack_sender::AckSender;
use crate::interface::connection::LongConnectionHandler;
use crate::interface::gateway::UnifiedGatewayHandler;
use crate::interface::grpc::handler::{AccessGatewayHandler, UnifiedGatewayGrpcHandler};
use crate::service::service_manager::PortConfig;
use tokio::sync::Mutex;

use flare_core::server::connection::ConnectionManager;
use flare_core::server::handle::{DefaultServerHandle, ServerHandle};
use flare_core::server::builder::flare::{FlareServerBuilder, FlareServer};
use flare_core::common::message::{ArcMessageMiddleware, LoggingMiddleware, MetricsMiddleware, ValidationMiddleware, LogLevel};
use flare_im_core::metrics::AccessGatewayMetrics;
use flare_server_core::auth::{RedisTokenStore, TokenService};
use flare_server_core::Config;

/// gRPC 服务集合
pub struct GrpcServices {
    pub signaling_handler: Arc<UnifiedGatewayGrpcHandler>,
    pub access_gateway_handler: Arc<AccessGatewayHandler>,
    pub grpc_addr: std::net::SocketAddr,
}

/// 应用上下文 - 包含所有已初始化的服务
pub struct ApplicationContext {
    pub long_connection_server: Arc<tokio::sync::Mutex<Option<FlareServer>>>,
    pub grpc_services: GrpcServices,
    /// 推送领域服务（用于批量消息刷新）
    pub push_domain_service: Arc<crate::domain::service::push_domain_service::PushDomainService>,
    /// 网关 ID
    pub gateway_id: String,
    /// 地区
    pub region: Option<String>,
}

/// 构建应用上下文
///
/// 类似 Go Wire 的 Initialize 函数，按照依赖顺序构建所有组件
///
/// # 参数
/// * `app_config` - 应用配置
/// * `runtime_config` - 运行时配置
/// * `port_config` - 端口配置
///
/// # 返回
/// * `ApplicationContext` - 构建好的应用上下文
pub async fn initialize(
    app_config: &flare_im_core::config::FlareAppConfig,
    runtime_config: &Config,
    port_config: PortConfig,
) -> Result<ApplicationContext> {
    use tracing::{error, info, warn};
    
    // 1. 加载配置
    let access_config = Arc::new(AccessGatewayConfig::from_app_config(app_config));
    
    // 2. 获取 gateway_id 和 region
    let gateway_id = access_config.gateway_id.clone().unwrap_or_else(|| {
        format!("gateway-{}", Uuid::new_v4().to_string()[..8].to_string())
    });
    let region = access_config.region.clone();
    
    info!("   Gateway ID: {}", gateway_id);
    if let Some(ref region) = region {
        info!("   Region: {}", region);
    }
    
    // 3. 初始化指标
    let metrics = Arc::new(AccessGatewayMetrics::new());
    info!("✅ Prometheus 指标已初始化");
    
    // 4. 构建连接管理器
    let connection_manager = Arc::new(ConnectionManager::new());
    
    // 5. 创建 Signaling 服务发现（使用常量，支持环境变量覆盖）
    use flare_im_core::service_names::{SIGNALING_ONLINE, MESSAGE_ORCHESTRATOR, SIGNALING_ROUTE, get_service_name};
    let signaling_service = get_service_name(SIGNALING_ONLINE);
    let signaling_discover = flare_im_core::discovery::create_discover(&signaling_service)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create signaling service discover for {}: {}", signaling_service, e))?;
    
    let signaling_service_client = if let Some(discover) = signaling_discover {
        Some(flare_server_core::discovery::ServiceClient::new(discover))
    } else {
        None
    };
    
    // 7. 构建 Signaling Gateway
    let signaling_gateway: Arc<dyn SignalingGateway> = if let Some(service_client) = signaling_service_client {
        Arc::new(GrpcSignalingGateway::with_service_client(signaling_service.clone(), service_client))
    } else {
        // 降级：使用服务名称（如果没有配置服务发现）
        Arc::new(GrpcSignalingGateway::new(signaling_service.clone()))
    };
    
    // 8. 构建连接查询服务
    let connection_query = build_connection_query(connection_manager.clone()).await;
    
    // 9. 构建ACK发布器（使用 gRPC，通过 Push Proxy 路由，支持跨区域部署）
    let ack_publisher: Option<Arc<dyn AckPublisher>> = if access_config.use_ack_report {
        // 使用 Push Proxy 接收 ACK 上报（跨区域部署时，Push Proxy 在本地区域，延迟更低）
        // ACK 通过 Push Proxy → Kafka → Push Server，保持架构一致性
        use flare_im_core::service_names::{PUSH_PROXY, get_service_name};
        let push_proxy_service_name = get_service_name(PUSH_PROXY);
        
        // 创建服务发现工厂
        let discovery_factory = Arc::new(flare_server_core::discovery::DiscoveryFactory {});
        let publisher = GrpcAckPublisher::new(discovery_factory, push_proxy_service_name.clone());
        info!("✅ ACK Publisher initialized: service={} (gRPC via Push Proxy)", push_proxy_service_name);
        Some(publisher)
    } else {
        info!("ACK Publisher disabled by configuration");
        None
    };
    
    // 10. 创建 Message Orchestrator 服务发现（用于直接路由模式，使用常量）
    let message_service = get_service_name(MESSAGE_ORCHESTRATOR);
    let message_service_discover = flare_im_core::discovery::create_discover(&message_service)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create message service discover for {}: {}", message_service, e))?;
    
    // 11. 构建消息路由服务
    let message_router: Option<Arc<crate::infrastructure::messaging::message_router::MessageRouter>> = {
        let service_name = message_service.clone();
        let default_tenant_id = "default".to_string();
        let default_svid = access_config.default_svid.clone();
        
        let router = if let Some(discover) = message_service_discover {
            let service_client = flare_server_core::discovery::ServiceClient::new(discover);
            Arc::new(
                crate::infrastructure::messaging::message_router::MessageRouter::with_service_client(
                    service_client,
                    default_tenant_id,
                    default_svid,
                )
            )
        } else {
            // 降级：使用服务名称（如果没有配置服务发现）
            Arc::new(
                crate::infrastructure::messaging::message_router::MessageRouter::new(
                    service_name.clone(),
                    default_tenant_id,
                    default_svid,
                )
            )
        };
        
        // 同步初始化连接（使用超时避免阻塞）
        let router_clone = router.clone();
        info!(service_name = %service_name, "正在初始化 Message Router...");
        match tokio::time::timeout(
            std::time::Duration::from_secs(10),
            router_clone.initialize()
        ).await {
            Ok(Ok(_)) => {
                info!(service_name = %service_name, "✅ Message Router initialized successfully");
            }
            Ok(Err(e)) => {
                error!(
                    error = %e,
                    service_name = %service_name,
                    "❌ Failed to initialize Message Router, will retry on first message"
                );
            }
            Err(_) => {
                warn!(
                    service_name = %service_name,
                    "⏱️ Message Router initialization timeout (10s), will retry on first message"
                );
            }
        }
        
        Some(router)
    };
    
    // 12. 构建连接处理器（提前构建，用于后续服务）
    let mut connection_handler = Arc::new(LongConnectionHandler::new_with_placeholders(
        signaling_gateway.clone(),
        gateway_id.clone(),
        ack_publisher.clone(),
        message_router.clone(),
        metrics.clone(),
    ));
    
    // 13. 构建领域服务
    let gateway_service_config = crate::domain::service::GatewayServiceConfig {
        gateway_id: gateway_id.clone(),
        online_service_endpoint: Some("http://127.0.0.1:50061".to_string()), // 添加Online服务端点
    };
    
    let gateway_service = Arc::new(GatewayService::new(
        signaling_gateway.clone(),
        connection_query.clone(),
        connection_handler.clone(),
        gateway_service_config,
    ).await);

    // 14. 构建会话领域服务（新增）
    let session_domain_service = Arc::new(SessionDomainService::new(
        signaling_gateway.clone(),
        Arc::new(crate::domain::service::connection_quality_service::ConnectionQualityService::new()),
        gateway_id.clone(),
    ));
    
    // 15. 构建应用服务
    let session_command_service = Arc::new(SessionCommandService::new(
        signaling_gateway.clone(),
        gateway_id.clone(),
    ));
    let session_query_service = Arc::new(SessionQueryService::new(signaling_gateway.clone()));
    
    // 16. 构建应用层服务（新增）
    let connection_app_service = Arc::new(ConnectionApplicationService::new(
        session_domain_service.clone(),
        connection_query.clone(),
        metrics.clone(),
    ));
    
    let ack_sender = Arc::new(AckSender::new(Arc::new(Mutex::new(None))));
    let message_app_service = Arc::new(MessageApplicationService::new(
        message_router.clone(),
        ack_sender.clone(),
        ack_publisher.clone(),
        session_domain_service.clone(),
        None, // session_service_client
        gateway_id.clone(),
    ));
    
    // 17. 更新连接处理器中的应用服务引用
    // 由于Rust的所有权机制和Arc的限制，我们不能直接替换connection_handler
    // 我们需要重新构建一个新的Arc来包含所有必要的服务
    let connection_handler = Arc::new(LongConnectionHandler::new(
        signaling_gateway.clone(),
        gateway_id.clone(),
        ack_publisher.clone(),
        message_router.clone(),
        metrics.clone(),
        connection_app_service.clone(),
        message_app_service.clone(),
    ));
    
    // 18. 构建推送领域服务
    let push_domain_service = Arc::new(PushDomainService::new(
        connection_handler.clone(),
        connection_query.clone(),
    ));
    
    // 19. 构建推送服务（应用层）
    let push_service = Arc::new(PushMessageService::new(
        push_domain_service.clone(),
        ack_publisher.clone(),
        gateway_id.clone(),
        metrics.clone(),
    ));
    let connection_query_service = Arc::new(ConnectionQueryService::new(connection_query.clone()));
    
    // 20. 构建处理器
    let handler = Arc::new(UnifiedGatewayHandler::new(
        session_command_service.clone(),
        session_query_service.clone(),
        connection_handler.clone(),
    ));
    
    // 21. 构建认证器
    let authenticator = build_authenticator(&access_config).await;
    
    // 22. 构建长连接服务器
    info!("准备构建长连接服务器...");
    let long_connection_server = build_long_connection_server(
        runtime_config,
        port_config.ws_port,
        port_config.quic_port,
        handler.clone(),
        connection_manager.clone(),
        authenticator,
        connection_handler.clone(),
    )
    .await
    .context("Failed to build long connection server")?;
    
    info!("✅ 长连接服务器构建完成");
    
    // 逻辑移到了 build_long_connection_server 内部，避免 Start 后的 Race Condition
    // 这里不需要再做一次了
    
    // 24. 构建 gRPC 处理器
    info!("构建 gRPC 处理器...");
    let signaling_grpc_handler = Arc::new(UnifiedGatewayGrpcHandler::new(
        handler.clone(),
    ));
    
    let access_gateway_grpc_handler = Arc::new(AccessGatewayHandler::new(
        push_service.clone(),
        connection_query_service.clone(),
        gateway_service.subscription_service.clone(),
        connection_handler.clone(),
    ));    info!("✅ gRPC 处理器构建完成");
    
    // 25. gRPC 地址
    let grpc_addr: std::net::SocketAddr = format!(
        "{}:{}",
        runtime_config.server.address, port_config.grpc_port
    )
    .parse()
    .context("Invalid gRPC address")?;
    
    info!("✅ 应用上下文构建完成，准备返回");
    Ok(ApplicationContext {
        long_connection_server,
        grpc_services: GrpcServices {
            signaling_handler: signaling_grpc_handler,
            access_gateway_handler: access_gateway_grpc_handler,
            grpc_addr,
        },
        push_domain_service: push_domain_service.clone(),
        gateway_id,
        region,
    })
}


/// 构建连接查询
async fn build_connection_query(
    connection_manager: Arc<ConnectionManager>,
) -> Arc<dyn ConnectionQuery> {
    Arc::new(ManagerConnectionQuery::new(connection_manager as Arc<_>)) as Arc<dyn ConnectionQuery>
}

/// 构建认证器
async fn build_authenticator(
    config: &AccessGatewayConfig,
) -> Arc<dyn flare_core::server::auth::Authenticator + Send + Sync> {
    use tracing::warn;
    
    let mut token_service = TokenService::new(
        config.token_secret.clone(),
        config.token_issuer.clone(),
        config.token_ttl_seconds,
    );
    
    if let Some(store_url) = &config.token_store_redis_url {
        match RedisTokenStore::new(store_url) {
            Ok(store) => {
                token_service = token_service.with_store(Arc::new(store));
            }
            Err(err) => warn!(
                ?err,
                "failed to initialize token store, proceeding without revocation support"
            ),
        }
    }
    
    Arc::new(TokenAuthenticator::new(Arc::new(token_service)))
        as Arc<dyn flare_core::server::auth::Authenticator + Send + Sync>
}

/// 构建长连接服务器
async fn build_long_connection_server(
    runtime_config: &Config,
    ws_port: u16,
    quic_port: u16,
    handler: Arc<UnifiedGatewayHandler>,
    connection_manager: Arc<ConnectionManager>,
    authenticator: Arc<dyn flare_core::server::auth::Authenticator + Send + Sync>,
    connection_handler: Arc<LongConnectionHandler>,
) -> Result<Arc<tokio::sync::Mutex<Option<FlareServer>>>> {
    use tracing::{error, info};
    use std::io::Write;
    
    // 创建中间件
    let validation_middleware = Arc::new(ValidationMiddleware::new(
        "GatewayValidation",
        |frame: &flare_core::common::protocol::Frame| -> flare_core::common::error::Result<()> {
            // 验证消息 ID 不为空
            if frame.message_id.is_empty() {
                return Err(flare_core::common::error::FlareError::message_format_error("Message ID is empty".to_string()));
            }
            Ok(())
        },
    )) as ArcMessageMiddleware;
    
    let logging_middleware = Arc::new(LoggingMiddleware::new("GatewayLogging")
        .with_level(LogLevel::Info)) as ArcMessageMiddleware;
    
    let metrics_middleware = Arc::new(MetricsMiddleware::new("GatewayMetrics")) as ArcMessageMiddleware;
    
    // 创建设备管理器（用于设备冲突管理）
    use flare_core::server::device::DeviceManager;
    use flare_core::common::device::DeviceConflictStrategyBuilder;
    let device_manager = Arc::new(DeviceManager::new(
        DeviceConflictStrategyBuilder::new()
            .platform_exclusive() // 平台互斥：同一用户同一平台只能有一个设备在线
            .build()
    ));
    
    // 使用 FlareServerBuilder 构建服务器，并在 QUIC 端口冲突时降级为仅 WebSocket
    let ws_addr = format!("{}:{}", runtime_config.server.address, ws_port);
    let quic_addr = format!("{}:{}", runtime_config.server.address, quic_port);

    let builder_with_quic = FlareServerBuilder::new(ws_addr.clone())
        .with_listener(connection_handler.clone())
        .with_middleware(validation_middleware.clone())
        .with_middleware(logging_middleware.clone())
        .with_middleware(metrics_middleware.clone())
        .with_connection_manager(connection_manager.clone())
        .with_device_manager(device_manager.clone())
        .enable_auth()
        .with_authenticator(authenticator.clone())
        .with_auth_timeout(Duration::from_secs(30))
        .with_max_connections(10000)
        .with_connection_timeout(Duration::from_secs(60))
        .with_heartbeat(flare_core::common::config_types::HeartbeatConfig {
            interval: Duration::from_secs(30),
            timeout: Duration::from_secs(90),
            enabled: true,
        })
        .with_default_format(flare_core::common::protocol::SerializationFormat::Protobuf)
        .with_default_compression(flare_core::common::compression::CompressionAlgorithm::None)
        .with_protocols(vec![
            flare_core::common::config_types::TransportProtocol::WebSocket,
            flare_core::common::config_types::TransportProtocol::QUIC,
        ])
        .with_protocol_address(
            flare_core::common::config_types::TransportProtocol::WebSocket,
            ws_addr.clone(),
        )
        .with_protocol_address(
            flare_core::common::config_types::TransportProtocol::QUIC,
            quic_addr.clone(),
        );

    let server = match builder_with_quic.build() {
        Ok(server) => server,
        Err(e) => {
            let es = format!("{}", e);
            error!(error = %e, "Failed to build FlareServer with QUIC");
            if es.contains("Address already in use") || es.contains("创建 QUIC 端点失败") {
                use tracing::warn;
                warn!(
                    %quic_addr,
                    "QUIC 端口被占用，降级为仅 WebSocket 模式"
                );
                let builder_ws_only = FlareServerBuilder::new(ws_addr.clone())
                    .with_listener(connection_handler.clone())
                    .with_middleware(validation_middleware.clone())
                    .with_middleware(logging_middleware.clone())
                    .with_middleware(metrics_middleware.clone())
                    .with_connection_manager(connection_manager.clone())
                    .with_device_manager(device_manager.clone())
                    .enable_auth()
                    .with_authenticator(authenticator.clone())
                    .with_auth_timeout(Duration::from_secs(30))
                    .with_max_connections(10000)
                    .with_connection_timeout(Duration::from_secs(60))
                    .with_heartbeat(flare_core::common::config_types::HeartbeatConfig {
                        interval: Duration::from_secs(30),
                        timeout: Duration::from_secs(90),
                        enabled: true,
                    })
                    .with_default_format(flare_core::common::protocol::SerializationFormat::Protobuf)
                    .with_default_compression(flare_core::common::compression::CompressionAlgorithm::None)
                    .with_protocols(vec![
                        flare_core::common::config_types::TransportProtocol::WebSocket,
                    ])
                    .with_protocol_address(
                        flare_core::common::config_types::TransportProtocol::WebSocket,
                        ws_addr.clone(),
                    );
                builder_ws_only.build().map_err(|e2| {
                    error!(error = %e2, "Failed to build FlareServer (WebSocket only)");
                    anyhow::anyhow!("Failed to build server: {}", e2)
                })?
            } else {
                return Err(anyhow::anyhow!("Failed to build server: {}", e));
            }
        }
    };
    
    // 23. 获取 server handle 和 connection manager
    // 直接使用传入的 connection_manager，避免从 server 获取可能导致的类型或所有权问题
    info!("设置 server handle 和 connection manager...");
    
    // ConnectionManager 实现了 ConnectionManagerTrait，可以直接转型
    let manager_trait: Arc<dyn flare_core::server::connection::ConnectionManagerTrait> = connection_manager.clone();
    
    // 创建 ServerHandle
    let server_handle: Arc<dyn ServerHandle> = Arc::new(DefaultServerHandle::new(manager_trait.clone()));
    
    handler.set_server_handle(server_handle).await;
    handler.set_connection_manager(manager_trait).await;
    info!("✅ Server handle 和 connection manager 设置完成");
    
    // 24. 启动长连接服务器 (移到依赖注入之后)
    info!("正在启动长连接服务器...");
    let _ = std::io::stdout().flush();
    
    server.start().await.map_err(|e| {
        error!(error = %e, "Failed to start FlareServer");
        eprintln!("❌ 启动长连接服务器失败: {}", e);
        anyhow::anyhow!("Failed to start server: {}", e)
    })?;
    
    info!("✅ 长连接服务器已启动");
    info!("   WebSocket: ws://{} 或 wss://{}", ws_addr, ws_addr);
    info!("   QUIC:      quic://{}", quic_addr);
    
    let _ = std::io::stdout().flush();
    
    Ok(Arc::new(tokio::sync::Mutex::new(Some(server))))
}
