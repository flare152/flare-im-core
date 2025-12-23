//! Wire 风格的依赖注入模块
//!
//! 类似 Go 的 Wire 框架，提供简单的依赖构建方法

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use uuid::Uuid;

use crate::application::handlers::{
    ConnectionQueryService, PushMessageService,
};
use crate::application::handlers::{ConnectionHandler, MessageHandler};
use crate::config::AccessGatewayConfig;
use crate::domain::repository::{ConnectionQuery, SignalingGateway};
use crate::domain::service::{GatewayService, PushDomainService, ConversationDomainService, MessageDomainService};
use crate::infrastructure::auth::TokenAuthenticator;
use crate::infrastructure::connection_query::ManagerConnectionQuery;
use crate::infrastructure::messaging::ack_sender::AckSender;
use crate::infrastructure::signaling::grpc::GrpcSignalingGateway;
use crate::infrastructure::{AckPublisher, GrpcAckPublisher};
use crate::interface::handler::LongConnectionHandler;
use crate::interface::grpc::handler::AccessGatewayHandler;
use crate::service::service_manager::PortConfig;
use tokio::sync::Mutex;

// 注意：最新的 Flare 模式不再需要在 FlareServerBuilder 中配置中间件
// 中间件是客户端特性，服务端通过 ServerEventHandler 处理消息
use flare_core::server::builder::flare::{FlareServer, FlareServerBuilder};
use flare_core::server::connection::ConnectionManager;
use flare_core::server::handle::{DefaultServerHandle, ServerHandle};
use flare_im_core::metrics::AccessGatewayMetrics;
use flare_server_core::Config;
use flare_server_core::auth::{RedisTokenStore, TokenService};

/// gRPC 服务集合
///
pub struct GrpcServices {
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
    use tracing::{debug, error, info};

    // 1. 加载配置
    let access_config = Arc::new(AccessGatewayConfig::from_app_config(app_config));

    // 2. 获取 gateway_id 和 region
    let gateway_id = access_config
        .gateway_id
        .clone()
        .unwrap_or_else(|| format!("gateway-{}", &Uuid::new_v4().to_string()[..8]));
    let region = access_config.region.clone();

    info!(gateway_id = %gateway_id, "Gateway initialized");
    if let Some(ref r) = region {
        info!(region = %r, "Gateway region configured");
    }

    // 3. 初始化指标
    let metrics = Arc::new(AccessGatewayMetrics::new());
    debug!("Prometheus metrics initialized");

    // 4. 构建连接管理器
    let connection_manager = Arc::new(ConnectionManager::new());

    // 5. 创建 Signaling 服务发现（使用常量，支持环境变量覆盖）
    use flare_im_core::service_names::{SIGNALING_ONLINE, get_service_name};
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

    let signaling_service_client =
        signaling_discover.map(flare_server_core::discovery::ServiceClient::new);

    // 6. 构建 Signaling Gateway
    let signaling_gateway: Arc<dyn SignalingGateway> =
        if let Some(service_client) = signaling_service_client {
            Arc::new(GrpcSignalingGateway::with_service_client(
                signaling_service.clone(),
                service_client,
            ))
        } else {
            // 降级：使用服务名称（如果没有配置服务发现）
            Arc::new(GrpcSignalingGateway::new(signaling_service.clone()))
        };

    // 7. 构建连接查询服务
    let connection_query = build_connection_query(connection_manager.clone()).await;

    // 8. 构建ACK发布器（使用 gRPC，通过 Push Proxy 路由，支持跨区域部署）
    let ack_publisher: Option<Arc<dyn AckPublisher>> = if access_config.use_ack_report {
        // 使用 Push Proxy 接收 ACK 上报（跨区域部署时，Push Proxy 在本地区域，延迟更低）
        // ACK 通过 Push Proxy → Kafka → Push Server，保持架构一致性
        use flare_im_core::service_names::{PUSH_PROXY, get_service_name};
        let push_proxy_service_name = get_service_name(PUSH_PROXY);

        // 创建服务发现工厂
        let discovery_factory = Arc::new(flare_server_core::discovery::DiscoveryFactory {});
        let publisher = GrpcAckPublisher::new(discovery_factory, push_proxy_service_name.clone());
        info!(service = %push_proxy_service_name, "ACK Publisher initialized (gRPC via Push Proxy)");
        Some(publisher)
    } else {
        debug!("ACK Publisher disabled by configuration");
        None
    };

    // 9. 创建 Route 服务发现（用于消息路由，使用常量）
    use flare_im_core::service_names::SIGNALING_ROUTE;
    let route_service = get_service_name(SIGNALING_ROUTE);
    let route_service_discover = flare_im_core::discovery::create_discover(&route_service)
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to create Route service discover for {}: {}",
                route_service,
                e
            )
        })?;

    // 10. 构建消息路由服务（通过 Route 服务路由消息）
    let message_router: Option<
        Arc<crate::infrastructure::messaging::message_router::MessageRouter>,
    > = {
        let service_name = route_service.clone();
        let default_tenant_id = "default".to_string();
        let default_svid = access_config.default_svid.clone();

        let router = if let Some(discover) = route_service_discover {
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
                ),
            )
        };

        // 同步初始化连接（使用超时避免阻塞）
        info!(route_service = %service_name, "Initializing Message Router (via Route Service)");
        match tokio::time::timeout(Duration::from_secs(10), router.initialize()).await {
            Ok(Ok(_)) => {
                info!(route_service = %service_name, "Message Router initialized successfully");
            }
            Ok(Err(e)) => {
                error!(
                    error = %e,
                    route_service = %service_name,
                    "Failed to initialize Message Router, will retry on first message"
                );
            }
            Err(_) => {
                error!(
                    route_service = %service_name,
                    "Message Router initialization timeout (10s), will retry on first message"
                );
            }
        }

        Some(router)
    };

    // 11. 构建连接处理器（提前构建，用于后续服务）
    let connection_handler = Arc::new(LongConnectionHandler::new_with_placeholders(
        signaling_gateway.clone(),
        gateway_id.clone(),
        ack_publisher.clone(),
        message_router.clone(),
        metrics.clone(),
    ));

    // 12. 构建领域服务
    let gateway_service_config = crate::domain::service::GatewayServiceConfig {
        gateway_id: gateway_id.clone(),
        online_service_endpoint: Some("http://127.0.0.1:50061".to_string()),
    };

    let gateway_service = Arc::new(
        GatewayService::new(
            signaling_gateway.clone(),
            connection_query.clone(),
            connection_handler.clone(),
            gateway_service_config,
        )
        .await,
    );

    // 13. 构建会话领域服务
    let session_domain_service = Arc::new(ConversationDomainService::new(
        signaling_gateway.clone(),
        Arc::new(
            crate::domain::service::connection_quality_service::ConnectionQualityService::new(),
        ),
        gateway_id.clone(),
    ));

    // 15. 构建领域服务
    let message_domain_service = Arc::new(MessageDomainService::new());

    // 16. 构建应用层处理器（只负责编排，业务逻辑在领域层）
    let connection_handler_app = Arc::new(ConnectionHandler::new(
        session_domain_service.clone(),
        connection_query.clone(),
        metrics.clone(),
    ));

    let ack_sender = Arc::new(AckSender::new(Arc::new(Mutex::new(None))));
    let message_handler_app = Arc::new(MessageHandler::new(
        message_domain_service,
        message_router.clone(),
        ack_sender.clone(),
        ack_publisher.clone(),
        session_domain_service.clone(),
        None, // conversation_service_client
        gateway_id.clone(),
    ));

    // 16. 更新连接处理器中的应用处理器引用
    let connection_handler = Arc::new(LongConnectionHandler::new(
        signaling_gateway.clone(),
        gateway_id.clone(),
        ack_publisher.clone(),
        message_router.clone(),
        metrics.clone(),
        connection_handler_app.clone(),
        message_handler_app.clone(),
    ));

    // 17. 构建推送领域服务
    let push_domain_service = Arc::new(PushDomainService::new(
        connection_handler.clone(),
        connection_query.clone(),
    ));

    // 18. 构建推送服务（应用层）
    let push_service = Arc::new(PushMessageService::new(
        push_domain_service.clone(),
        ack_publisher.clone(),
        gateway_id.clone(),
        metrics.clone(),
    ));
    let connection_query_service = Arc::new(ConnectionQueryService::new(connection_query.clone()));

    // 19. 构建认证器
    let authenticator = build_authenticator(&access_config).await;

    // 20. 构建长连接服务器
    debug!(ws_port = %port_config.ws_port, quic_port = %port_config.quic_port, "Building long connection server");
    let long_connection_server = build_long_connection_server(
        runtime_config,
        port_config.ws_port,
        port_config.quic_port,
        connection_manager.clone(),
        authenticator,
        connection_handler.clone(),
        access_config.clone(),
    )
    .await
    .context("Failed to build long connection server")?;

    info!("Long connection server built successfully");

    // 21. 构建 gRPC 处理器
    // 注意：SignalingService 由 flare-signaling/online 服务实现，Gateway 不再提供
    debug!("Building gRPC handlers");

    let access_gateway_grpc_handler = Arc::new(AccessGatewayHandler::new(
        push_service.clone(),
        connection_query_service.clone(),
        gateway_service.subscription_service.clone(),
        connection_handler.clone(),
    ));
    debug!("gRPC handlers built successfully");

    // 22. gRPC 地址
    let grpc_addr = format!(
        "{}:{}",
        runtime_config.server.address, port_config.grpc_port
    )
    .parse::<std::net::SocketAddr>()
    .context("Invalid gRPC address")?;

    info!("Application context initialized successfully");
    Ok(ApplicationContext {
        long_connection_server,
        grpc_services: GrpcServices {
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
    Arc::new(ManagerConnectionQuery::new(connection_manager))
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
            Err(err) => {
                warn!(
                    ?err,
                    "Failed to initialize token store, proceeding without revocation support"
                );
            }
        }
    }

    Arc::new(TokenAuthenticator::new(Arc::new(token_service)))
}

/// 使用 Flare 模式构建服务器
///
/// Flare 模式特点：
/// - 只需实现 `ServerEventHandler` trait
/// - 自动消息路由和 ACK 处理
/// - 支持设备管理、认证、多协议等完整功能
fn build_flare_server(
    ws_addr: String,
    quic_addr: Option<String>,
    connection_handler: Arc<LongConnectionHandler>,
    connection_manager: Arc<ConnectionManager>,
    device_manager: Arc<flare_core::server::device::DeviceManager>,
    authenticator: Arc<dyn flare_core::server::auth::Authenticator + Send + Sync>,
    compression_algorithm: flare_core::common::compression::CompressionAlgorithm,
    encryption_enabled: bool,
) -> Result<FlareServer> {
    use flare_core::common::config_types::{HeartbeatConfig, TransportProtocol};
    use flare_core::common::protocol::SerializationFormat;
    
    // LongConnectionHandler 实现了 ServerEventHandler，Flare 模式会自动路由消息
    let event_handler: Arc<dyn flare_core::server::events::handler::ServerEventHandler> = 
        connection_handler.clone();
    
    let mut builder = FlareServerBuilder::new(ws_addr.clone(), event_handler)
        // 连接和设备管理
        .with_connection_manager(connection_manager)
        .with_device_manager(device_manager)
        // 认证配置
        .enable_auth()
        .with_authenticator(authenticator)
        .with_auth_timeout(Duration::from_secs(30))
        // 连接配置
        .with_max_connections(10000)
        .with_connection_timeout(Duration::from_secs(60))
        .with_heartbeat(HeartbeatConfig {
            interval: Duration::from_secs(30),
            timeout: Duration::from_secs(90),
            enabled: true,
        })
        // 协商配置（使用配置的压缩算法）
        .with_default_format(SerializationFormat::Protobuf)
        .with_default_compression(compression_algorithm);
    
    // 可选：启用加密
    if encryption_enabled {
        builder = builder.with_default_encryption(
            flare_core::common::encryption::EncryptionAlgorithm::Aes256Gcm
        );
    }
    
    // 协议配置
    if let Some(quic) = quic_addr {
        builder = builder
            .with_protocols(vec![TransportProtocol::WebSocket, TransportProtocol::QUIC])
            .with_protocol_address(TransportProtocol::WebSocket, ws_addr)
            .with_protocol_address(TransportProtocol::QUIC, quic);
    } else {
        builder = builder
            .with_protocols(vec![TransportProtocol::WebSocket])
            .with_protocol_address(TransportProtocol::WebSocket, ws_addr);
    }
    
    builder.build().map_err(|e| anyhow::anyhow!("Failed to build FlareServer: {}", e))
}

/// 构建长连接服务器
async fn build_long_connection_server(
    runtime_config: &Config,
    ws_port: u16,
    quic_port: u16,
    connection_manager: Arc<ConnectionManager>,
    authenticator: Arc<dyn flare_core::server::auth::Authenticator + Send + Sync>,
    connection_handler: Arc<LongConnectionHandler>,
    access_config: Arc<AccessGatewayConfig>,
) -> Result<Arc<tokio::sync::Mutex<Option<FlareServer>>>> {
    use tracing::{error, info, warn};

    // 创建设备管理器（平台互斥策略：同一用户同一平台只能有一个设备在线）
    use flare_core::common::device::DeviceConflictStrategyBuilder;
    use flare_core::server::device::DeviceManager;
    let device_manager = Arc::new(DeviceManager::new(
        DeviceConflictStrategyBuilder::new()
            .platform_exclusive()
            .build(),
    ));

    let ws_addr = format!("{}:{}", runtime_config.server.address, ws_port);
    let quic_addr = format!("{}:{}", runtime_config.server.address, quic_port);

    // 配置压缩和加密（从配置读取）
    info!(
        compression_algorithm = ?access_config.compression_algorithm,
        enable_encryption = %access_config.enable_encryption,
        "Reading compression and encryption configuration"
    );
    
    let compression_algorithm = parse_compression_algorithm(
        access_config.compression_algorithm.as_deref()
    );
    
    // 先注册加密器（如果启用），必须在构建服务器之前注册
    let encryption_config = setup_encryption_config(
        access_config.enable_encryption,
        access_config.encryption_key.as_deref(),
    ).await;
    
    info!(
        compression = ?compression_algorithm,
        encryption_enabled = %encryption_config.enabled,
        "Configuration parsed, building FlareServer"
    );

    // 尝试构建服务器（优先使用 QUIC + WebSocket）
    let server = match build_flare_server(
        ws_addr.clone(),
        Some(quic_addr.clone()),
        connection_handler.clone(),
        connection_manager.clone(),
        device_manager.clone(),
        authenticator.clone(),
        compression_algorithm.clone(),
        encryption_config.enabled,
    ) {
        Ok(server) => server,
        Err(e) => {
            let error_msg = e.to_string();
            // QUIC 端口被占用，降级为仅 WebSocket
            if error_msg.contains("Address already in use") 
                || error_msg.contains("创建 QUIC 端点失败") {
                warn!(quic_addr = %quic_addr, "QUIC port unavailable, falling back to WebSocket-only mode");
                build_flare_server(
                    ws_addr.clone(),
                    None, // 仅 WebSocket
                    connection_handler.clone(),
                    connection_manager.clone(),
                    device_manager.clone(),
                    authenticator.clone(),
                    compression_algorithm,
                    encryption_config.enabled,
                )?
            } else {
                error!(error = %e, "Failed to build FlareServer");
                return Err(anyhow::anyhow!("Failed to build server: {}", e));
            }
        }
    };

    // 设置 server handle 和 connection manager（用于消息发送和连接管理）
    setup_server_components(&connection_handler, &connection_manager).await;
    
    // 启动服务器
    server.start().await.map_err(|e| {
        error!(error = %e, "Failed to start FlareServer");
        anyhow::anyhow!("Failed to start server: {}", e)
    })?;

    info!(ws_addr = %ws_addr, quic_addr = %quic_addr, "✅ Long connection server started");

    Ok(Arc::new(tokio::sync::Mutex::new(Some(server))))
}

/// 加密配置
struct EncryptionConfig {
    enabled: bool,
}

/// 解析压缩算法
fn parse_compression_algorithm(algorithm: Option<&str>) -> flare_core::common::compression::CompressionAlgorithm {
    use flare_core::common::compression::CompressionAlgorithm;

    let result = match algorithm {
        Some("gzip") => CompressionAlgorithm::Gzip,
        Some("zstd") => CompressionAlgorithm::Zstd,
        Some("none") | Some("") | None => CompressionAlgorithm::None,
        Some(other) => {
            tracing::warn!(algorithm = %other, "Unknown compression algorithm, using None");
            CompressionAlgorithm::None
        }
    };
    
    tracing::debug!(algorithm = ?algorithm, parsed = ?result, "Parsed compression algorithm");
    result
}

/// 配置加密（如果启用）
async fn setup_encryption_config(
    enable_encryption: bool,
    encryption_key: Option<&str>,
) -> EncryptionConfig {
    if !enable_encryption {
        return EncryptionConfig { enabled: false };
    }

    use flare_core::common::encryption::{Aes256GcmEncryptor, EncryptionUtil};
    use tracing::{info, warn};

    // 解析加密密钥（32字节）
    let key_bytes = encryption_key.and_then(|key| {
        if key.len() == 32 {
            // 直接32字符的字符串
            Some(key.as_bytes().to_vec())
        } else if key.len() == 64 {
            // hex 编码的 64 字符字符串（32字节）
            (0..32).try_fold(Vec::new(), |mut acc, i| {
                u8::from_str_radix(&key[i * 2..i * 2 + 2], 16)
                    .map(|b| { acc.push(b); acc })
            }).ok()
        } else {
            None
        }
    });

    let encryption_key = key_bytes.unwrap_or_else(|| {
        warn!("Encryption key not set or invalid (expected 32 bytes or 64 hex chars), using default key (NOT SECURE FOR PRODUCTION)");
        b"01234567890123456789012345678901".to_vec() // 32 bytes for AES-256
    });

    match Aes256GcmEncryptor::new(&encryption_key) {
        Ok(encryptor) => {
            EncryptionUtil::register_custom(Arc::new(encryptor));
            info!("🔐 AES-256-GCM encryption enabled with custom key");
            EncryptionConfig { enabled: true }
        }
        Err(e) => {
            warn!(error = %e, "Failed to create encryption, encryption disabled");
            EncryptionConfig { enabled: false }
        }
        }
}

/// 设置服务器组件（ServerHandle 和 ConnectionManager）
async fn setup_server_components(
    connection_handler: &Arc<LongConnectionHandler>,
    connection_manager: &Arc<ConnectionManager>,
) {
    use tracing::info;

    let manager_trait: Arc<dyn flare_core::server::connection::ConnectionManagerTrait> =
        connection_manager.clone();
    let server_handle: Arc<dyn ServerHandle> =
        Arc::new(DefaultServerHandle::new(manager_trait.clone()));

    connection_handler.set_server_handle(server_handle).await;
    connection_handler.set_connection_manager(manager_trait).await;
    
    info!("✅ Server handle and connection manager configured");
}
