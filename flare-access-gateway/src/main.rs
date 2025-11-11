//! 统一接入网关服务
//!
//! 提供统一的网关服务，同时支持：
//! 1. gRPC 接口：认证、会话管理、在线状态查询
//! 2. WebSocket/QUIC 连接：客户端长连接、消息接收和推送
//!
//! ## 职责
//!
//! 1. **客户端连接接入**
//!    - 维护用户长连接（WebSocket/QUIC）
//!    - 支持多地区部署，方便不同地区用户就近接入
//!    - 单实例支持 20-50万 并发连接
//!
//! 2. **认证和授权**
//!    - 验证用户登录（uid/token）
//!    - 会话管理（session管理）
//!    - 设备管理
//!
//! 3. **心跳检测**
//!    - 接收用户心跳包
//!    - 检查连接状态
//!    - 自动清理失效连接
//!
//! 4. **消息处理**
//!    - 接收客户端消息
//!    - 推送消息到客户端
//!    - 消息路由和转发
//!
//! 5. **连接状态同步**
//!    - 将连接状态同步到 Online 服务
//!    - 支持多实例部署，状态共享
//!
//! ## 部署架构
//!
//! 统一网关需要部署在不同地区，方便用户就近接入：
//! - 北京机房：服务华北地区用户
//! - 上海机房：服务华东地区用户
//! - 广州机房：服务华南地区用户
//! - 香港机房：服务海外用户
//!
//! 每个地区可以部署多个实例，实现负载均衡和高可用。

use std::sync::Arc;

use flare_access_gateway::application::commands::SessionCommandService;
use flare_access_gateway::application::queries::SessionQueryService;
use flare_access_gateway::config::AccessGatewayConfig;
use flare_access_gateway::domain::SessionStore;
use flare_access_gateway::infrastructure::auth::TokenAuthenticator;
use flare_access_gateway::infrastructure::session_store::in_memory::InMemorySessionStore;
use flare_access_gateway::infrastructure::session_store::redis::RedisSessionStore;
use flare_access_gateway::infrastructure::signaling::grpc::GrpcSignalingGateway;
use flare_access_gateway::interface::connection::LongConnectionHandler;
use flare_access_gateway::interface::events::GatewayEventHandler;
use flare_access_gateway::interface::gateway::UnifiedGatewayHandler;
use flare_access_gateway::interface::grpc::server::UnifiedGatewayServer;
use flare_core::common::error::{FlareError, Result};
use flare_core::server::ObserverServerBuilder;
use flare_core::server::connection::ConnectionManager;
use flare_core::server::handle::{DefaultServerHandle, ServerHandle};
use flare_im_core::{load_config, register_service};
use flare_server_core::auth::{RedisTokenStore, TokenService};
use std::time::Duration;
use tokio::task;
use tracing::{info, warn};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let app_config = load_config(None);
    let service_cfg = app_config.access_gateway_service();
    let runtime_config =
        app_config.compose_service_config(&service_cfg.runtime, "flare-access-gateway");
    let service_type = runtime_config.service.name.clone();

    info!("🚀 启动统一接入网关服务");
    info!("");
    info!("📋 服务说明：");
    info!("   - 提供 gRPC 接口：认证、会话管理");
    info!("   - 提供 WebSocket/QUIC 连接：客户端长连接、消息传输");
    info!("   - 支持多地区部署，就近接入");
    info!("");

    let _registry = register_service(&runtime_config, &service_type)
        .await
        .map_err(|err| FlareError::system(err.to_string()))?;
    info!("✅ 服务已注册到注册中心");

    let ws_port = runtime_config.server.port;
    let quic_port = runtime_config.server.port + 1;
    let grpc_port = runtime_config.server.port + 2;

    let access_config = Arc::new(AccessGatewayConfig::from_app_config(app_config));
    let gateway_id = format!("gateway-{}", Uuid::new_v4().to_string()[..8].to_string());

    let connection_manager = Arc::new(ConnectionManager::new());

    let session_store: Arc<dyn SessionStore> = if let Some(redis_url) =
        &access_config.session_store_redis_url
    {
        match redis::Client::open(redis_url.as_str()) {
            Ok(client) => {
                let client = Arc::new(client);
                Arc::new(RedisSessionStore::new(
                    client,
                    access_config.session_store_ttl_seconds,
                )) as Arc<dyn SessionStore>
            }
            Err(err) => {
                warn!(?err, %redis_url, "failed to initialize redis session store, falling back to memory");
                Arc::new(InMemorySessionStore::new())
            }
        }
    } else {
        Arc::new(InMemorySessionStore::new())
    };

    let signaling_gateway = GrpcSignalingGateway::new(access_config.signaling_endpoint.clone());

    let command_service = Arc::new(SessionCommandService::new(
        signaling_gateway.clone(),
        session_store.clone(),
        gateway_id.clone(),
    ));
    let query_service = Arc::new(SessionQueryService::new(signaling_gateway.clone()));

    let connection_handler = Arc::new(LongConnectionHandler::new(session_store.clone()));

    let handler = Arc::new(UnifiedGatewayHandler::new(
        command_service.clone(),
        query_service.clone(),
        connection_handler.clone(),
    ));

    let mut token_service = TokenService::new(
        access_config.token_secret.clone(),
        access_config.token_issuer.clone(),
        access_config.token_ttl_seconds,
    );

    if let Some(store_url) = &access_config.token_store_redis_url {
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

    let token_service = Arc::new(token_service);

    let authenticator = Arc::new(TokenAuthenticator::new(token_service.clone()))
        as Arc<dyn flare_core::server::auth::Authenticator + Send + Sync>;

    let event_handler = Arc::new(GatewayEventHandler::new(connection_handler.clone()));

    let mut long_connection_server =
        ObserverServerBuilder::new(format!("{}:{}", runtime_config.server.address, ws_port))
            .with_handler(handler.connection_handler())
            .with_connection_manager(connection_manager.clone())
            .enable_auth()
            .with_authenticator(authenticator)
            .with_auth_timeout(Duration::from_secs(30))
            .with_event_handler(event_handler)
            .with_protocols(vec![
                flare_core::common::config_types::TransportProtocol::WebSocket,
                flare_core::common::config_types::TransportProtocol::QUIC,
            ])
            .with_protocol_address(
                flare_core::common::config_types::TransportProtocol::WebSocket,
                format!("{}:{}", runtime_config.server.address, ws_port),
            )
            .with_protocol_address(
                flare_core::common::config_types::TransportProtocol::QUIC,
                format!("{}:{}", runtime_config.server.address, quic_port),
            )
            .with_max_connections(10000)
            .with_heartbeat(flare_core::common::config_types::HeartbeatConfig {
                interval: Duration::from_secs(30),
                timeout: Duration::from_secs(90),
                enabled: true,
            })
            .with_default_format(flare_core::common::protocol::SerializationFormat::Protobuf)
            .with_default_compression(flare_core::common::compression::CompressionAlgorithm::None)
            .build()?;

    let (server_handle, manager_trait) =
        if let Some(manager_trait) = long_connection_server.get_server_handle_components() {
            let handle: Arc<dyn ServerHandle> =
                Arc::new(DefaultServerHandle::new(manager_trait.clone()));
            (handle, manager_trait)
        } else {
            return Err("无法获取连接管理器".into());
        };
    handler.set_server_handle(server_handle).await;
    handler.set_connection_manager(manager_trait).await;

    let grpc_service = UnifiedGatewayServer::new(
        runtime_config.clone(),
        Arc::clone(&handler),
        connection_manager.clone(),
    );

    let grpc_addr: std::net::SocketAddr =
        format!("{}:{}", runtime_config.server.address, grpc_port)
            .parse()
            .map_err(|err: std::net::AddrParseError| FlareError::system(err.to_string()))?;

    long_connection_server.start().await?;

    info!("✅ 统一接入网关已启动");
    info!("   Gateway ID: {}", gateway_id);
    info!(
        "   WebSocket: ws://{}:{}",
        runtime_config.server.address, ws_port
    );
    info!(
        "   QUIC: quic://{}:{}",
        runtime_config.server.address, quic_port
    );
    info!(
        "   gRPC: http://{}:{}",
        runtime_config.server.address, grpc_port
    );

    let grpc_server_handle = task::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(
                flare_proto::signaling::signaling_service_server::SignalingServiceServer::new(
                    grpc_service,
                ),
            )
            .serve(grpc_addr)
            .await
    });

    tokio::signal::ctrl_c().await?;
    info!("\n正在停止服务器...");

    long_connection_server.stop().await?;
    grpc_server_handle.abort();

    info!("✅ 服务器已停止");

    Ok(())
}
