//! 应用启动器 - 负责依赖注入和服务启动

use crate::application::commands::{PushMessageService, SessionCommandService};
use crate::application::queries::{ConnectionQueryService, SessionQueryService};
use crate::application::service::GatewayApplication;
use crate::config::AccessGatewayConfig;
use crate::domain::repositories::{ConnectionQuery, SessionStore, SignalingGateway};
use crate::domain::service::GatewayService;
use crate::infrastructure::auth::TokenAuthenticator;
use crate::infrastructure::connection_query::ManagerConnectionQuery;
use crate::infrastructure::session_store::in_memory::InMemorySessionStore;
use crate::infrastructure::session_store::redis::RedisSessionStore;
use crate::infrastructure::signaling::grpc::GrpcSignalingGateway;
use crate::infrastructure::{AckPublisher, KafkaAckPublisher};
use crate::interface::connection::LongConnectionHandler;
use crate::interface::events::GatewayEventHandler;
use crate::interface::gateway::UnifiedGatewayHandler;
use crate::interface::grpc::access_gateway_server::AccessGatewayServer;
use crate::interface::grpc::server::UnifiedGatewayServer;
use crate::service::registry::ServiceRegistrar;
use anyhow::Result;
use flare_core::server::connection::ConnectionManager;
use flare_core::server::handle::{DefaultServerHandle, ServerHandle};
use flare_core::server::ObserverServerBuilder;
use flare_im_core::FlareAppConfig;
use flare_im_core::metrics::AccessGatewayMetrics;
use flare_server_core::auth::{RedisTokenStore, TokenService};
use flare_server_core::{ServiceInfo, ServiceRegistryTrait};
use std::sync::Arc;
use std::time::Duration;
use tokio::task;
use tonic::transport::Server;
use tracing::{info, warn};
use uuid::Uuid;

/// 应用上下文 - 包含所有已初始化的服务
pub struct ApplicationContext {
    pub long_connection_server: Arc<tokio::sync::Mutex<Option<flare_core::server::ObserverServer>>>,
    pub grpc_services: GrpcServices,
    pub service_registry: Option<Arc<tokio::sync::RwLock<Box<dyn ServiceRegistryTrait>>>>,
    pub service_info: Option<ServiceInfo>,
}

pub struct GrpcServices {
    pub signaling_server: Arc<UnifiedGatewayServer>,
    pub access_gateway_server: Arc<AccessGatewayServer>,
    pub grpc_addr: std::net::SocketAddr,
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
            if let Err(e) = flare_im_core::tracing::init_tracing("access-gateway", otlp_endpoint.as_deref()) {
                tracing::error!(error = %e, "Failed to initialize OpenTelemetry tracing");
            } else {
                info!("✅ OpenTelemetry tracing initialized");
            }
        }

        // 创建应用上下文
        let context = Self::create_context(config).await?;

        // 获取运行时配置
        let service_cfg = config.access_gateway_service();
        let runtime_config = config.compose_service_config(&service_cfg.runtime, "flare-access-gateway");

        let ws_port = runtime_config.server.port;
        let quic_port = runtime_config.server.port + 1;

        // 启动长连接服务器
        info!("🚀 启动统一接入网关服务");
        info!("");
        info!("📋 服务说明：");
        info!("   - 提供 gRPC 接口：认证、会话管理、消息推送");
        info!("   - 提供 WebSocket/QUIC 连接：客户端长连接、消息传输");
        info!("   - 支持多地区部署，就近接入");
        info!("");

        // 启动服务器
        Self::start_servers(context, &runtime_config.server.address, ws_port, quic_port).await
    }

    /// 创建应用上下文
    pub async fn create_context(config: &FlareAppConfig) -> Result<ApplicationContext> {
        let service_cfg = config.access_gateway_service();
        let runtime_config = config.compose_service_config(&service_cfg.runtime, "flare-access-gateway");
        let service_type = runtime_config.service.name.clone();
        let access_config = Arc::new(AccessGatewayConfig::from_app_config(config));

        // 注册服务
        let (service_registry, service_info) =
            ServiceRegistrar::register_service(&runtime_config, &service_type).await?;

        // 获取 gateway_id：优先使用配置，否则生成随机ID
        let gateway_id = access_config.gateway_id.clone().unwrap_or_else(|| {
            format!("gateway-{}", Uuid::new_v4().to_string()[..8].to_string())
        });
        let region = access_config.region.clone();
        
        info!("✅ 服务已注册到注册中心");
        info!("   Gateway ID: {}", gateway_id);
        if let Some(ref region) = region {
            info!("   Region: {}", region);
        }

        // 初始化指标
        let metrics = Arc::new(AccessGatewayMetrics::new());
        info!("✅ Prometheus 指标已初始化");

        // 构建基础设施
        let connection_manager = Arc::new(ConnectionManager::new());
        let session_store = Self::build_session_store(&access_config).await?;
        let signaling_gateway: Arc<dyn SignalingGateway> = Arc::new(GrpcSignalingGateway::new(access_config.signaling_endpoint.clone()));
        let connection_query = Self::build_connection_query(connection_manager.clone()).await;

        // 构建在线状态缓存
        let online_cache = Arc::new(
            crate::infrastructure::online_cache::OnlineStatusCache::new(
                access_config.online_cache_ttl_seconds,
                access_config.online_cache_max_size,
            )
        );

        // 构建ACK发布器
        let ack_publisher: Option<Arc<dyn AckPublisher>> = match (
            access_config.kafka_bootstrap.as_ref(),
            access_config.ack_topic.as_ref(),
        ) {
            (Some(bootstrap), Some(topic)) => {
                match KafkaAckPublisher::new(bootstrap, topic.clone()) {
                    Ok(publisher) => {
                        info!("✅ ACK Publisher initialized: topic={}", topic);
                        Some(publisher)
                    }
                    Err(e) => {
                        warn!(
                            ?e,
                            "Failed to initialize Kafka ACK Publisher, ACK reporting disabled"
                        );
                        None
                    }
                }
            }
            _ => {
                info!("ACK Publisher not configured, ACK reporting disabled");
                None
            }
        };

        // 构建消息路由服务
        let message_router: Option<Arc<crate::infrastructure::messaging::message_router::MessageRouter>> = {
            let endpoint = access_config.message_endpoint.clone();
            let default_tenant_id = "default".to_string(); // 默认租户ID
            let router = Arc::new(crate::infrastructure::messaging::message_router::MessageRouter::new(
                endpoint.clone(),
                default_tenant_id,
            ));
            
            // 异步初始化连接
            let router_clone = router.clone();
            tokio::spawn(async move {
                if let Err(e) = router_clone.initialize().await {
                    warn!(
                        ?e,
                        endpoint = %endpoint,
                        "Failed to initialize Message Router, message routing disabled"
                    );
                } else {
                    info!(endpoint = %endpoint, "✅ Message Router initialized");
                }
            });
            
            Some(router)
        };

        // 构建领域服务
        let gateway_service = Arc::new(GatewayService::new(
            session_store.clone(),
            signaling_gateway.clone(),
            connection_query.clone(),
        ));
        let application = Arc::new(GatewayApplication::new(gateway_service));

        // 构建应用服务
        let session_command_service = Arc::new(SessionCommandService::new(
            signaling_gateway.clone(),
            session_store.clone(),
            gateway_id.clone(),
        ));
        let session_query_service = Arc::new(SessionQueryService::new(signaling_gateway.clone()));

        let connection_handler = Arc::new(LongConnectionHandler::new(
            session_store.clone(),
            signaling_gateway.clone(),
            online_cache.clone(),
            gateway_id.clone(),
            ack_publisher.clone(),
            message_router.clone(),
            metrics.clone(),
        ));

        // 构建推送服务
        let push_service = Arc::new(PushMessageService::new(
            connection_handler.clone(),
            connection_query.clone(),
            online_cache.clone(),
            ack_publisher.clone(),
            gateway_id.clone(),
            metrics.clone(),
        ));
        let connection_query_service = Arc::new(ConnectionQueryService::new(connection_query.clone()));

        // 构建处理器
        let handler = Arc::new(UnifiedGatewayHandler::new(
            session_command_service.clone(),
            session_query_service.clone(),
            connection_handler.clone(),
        ));

        // 构建认证器
        let authenticator = Self::build_authenticator(&access_config).await;

        // 构建长连接服务器
        let ws_port = runtime_config.server.port;
        let quic_port = runtime_config.server.port + 1;
        let long_connection_server = Self::build_long_connection_server(
            &runtime_config,
            ws_port,
            quic_port,
            handler.clone(),
            connection_manager.clone(),
            authenticator,
            connection_handler.clone(),
        )
        .await?;

        // 获取 server handle 和 connection manager
        let server_guard = long_connection_server.lock().await;
        let server = server_guard.as_ref().ok_or_else(|| anyhow::anyhow!("Server not initialized"))?;
        let (server_handle, manager_trait) = if let Some(manager_trait) =
            server.get_server_handle_components()
        {
            let handle: Arc<dyn ServerHandle> =
                Arc::new(DefaultServerHandle::new(manager_trait.clone()));
            (handle, manager_trait)
        } else {
            return Err(anyhow::anyhow!("无法获取连接管理器"));
        };

        drop(server_guard); // 释放锁
        
        handler.set_server_handle(server_handle).await;
        handler.set_connection_manager(manager_trait).await;

        // 构建 gRPC 服务
        let signaling_grpc_service = Arc::new(UnifiedGatewayServer::new(
            runtime_config.clone(),
            handler.clone(),
            connection_manager.clone(),
        ));

        let access_gateway_grpc_service = Arc::new(AccessGatewayServer::new(
            push_service.clone(),
            connection_query_service.clone(),
        ));

        let grpc_port = runtime_config.server.port + 2;
        let grpc_addr: std::net::SocketAddr = format!(
            "{}:{}",
            runtime_config.server.address, grpc_port
        )
        .parse()
        .map_err(|err| anyhow::anyhow!("Invalid gRPC address: {}", err))?;

        info!("✅ 统一接入网关已启动");
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
        info!("   - SignalingService: gRPC 接口");
        info!("   - AccessGateway: gRPC 接口（业务系统推送消息）");

        Ok(ApplicationContext {
            long_connection_server,
            grpc_services: GrpcServices {
                signaling_server: signaling_grpc_service,
                access_gateway_server: access_gateway_grpc_service,
                grpc_addr,
            },
            service_registry,
            service_info,
        })
    }

    /// 构建会话存储
    async fn build_session_store(
        config: &AccessGatewayConfig,
    ) -> Result<Arc<dyn SessionStore>> {
        if let Some(redis_url) = &config.session_store_redis_url {
            match redis::Client::open(redis_url.as_str()) {
                Ok(client) => {
                    let client = Arc::new(client);
                    Ok(Arc::new(RedisSessionStore::new(
                        client,
                        config.session_store_ttl_seconds,
                    )) as Arc<dyn SessionStore>)
                }
                Err(err) => {
                    warn!(
                        ?err,
                        %redis_url,
                        "failed to initialize redis session store, falling back to memory"
                    );
                    Ok(Arc::new(InMemorySessionStore::new()))
                }
            }
        } else {
            Ok(Arc::new(InMemorySessionStore::new()))
        }
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
        runtime_config: &flare_server_core::Config,
        ws_port: u16,
        quic_port: u16,
        handler: Arc<UnifiedGatewayHandler>,
        connection_manager: Arc<ConnectionManager>,
        authenticator: Arc<dyn flare_core::server::auth::Authenticator + Send + Sync>,
        connection_handler: Arc<LongConnectionHandler>,
    ) -> Result<Arc<tokio::sync::Mutex<Option<flare_core::server::ObserverServer>>>> {
        let event_handler = Arc::new(GatewayEventHandler::new(connection_handler));

        let mut server = ObserverServerBuilder::new(format!(
            "{}:{}",
            runtime_config.server.address, ws_port
        ))
        .with_handler(handler.connection_handler())
        .with_connection_manager(connection_manager)
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
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to build server: {}", e))?;

        server.start().await.map_err(|e| anyhow::anyhow!("Failed to start server: {}", e))?;
        Ok(Arc::new(tokio::sync::Mutex::new(Some(server))))
    }

    /// 启动所有服务器
    pub async fn start_servers(
        context: ApplicationContext,
        _address: &str,
        _ws_port: u16,
        _quic_port: u16,
    ) -> Result<()> {
        // 启动 gRPC 服务器
        let grpc_addr = context.grpc_services.grpc_addr.clone();
        let signaling_server = context.grpc_services.signaling_server.clone();
        let access_gateway_server = context.grpc_services.access_gateway_server.clone();
        
        let grpc_server_handle = task::spawn(async move {
            Server::builder()
                .add_service(
                    flare_proto::signaling::signaling_service_server::SignalingServiceServer::new(
                        (*signaling_server).clone(),
                    ),
                )
                .add_service(
                    flare_proto::access_gateway::access_gateway_server::AccessGatewayServer::new(
                        (*access_gateway_server).clone(),
                    ),
                )
                .serve(grpc_addr)
                .await
        });

        // 等待停止信号
        tokio::signal::ctrl_c().await?;
        info!("\n正在停止服务器...");

        // 执行优雅停机
        Self::graceful_shutdown(context, grpc_server_handle).await;

        info!("✅ 服务器已停止");
        Ok(())
    }

    /// 优雅停机处理
    async fn graceful_shutdown(
        context: ApplicationContext,
        grpc_server_handle: task::JoinHandle<std::result::Result<(), tonic::transport::Error>>,
    ) {
        // 如果有服务注册器，执行服务注销
        if let (Some(registry), Some(service_info)) = (&context.service_registry, &context.service_info) {
            info!("unregistering service...");
            let mut registry = registry.write().await;
            if let Err(e) = registry.unregister(&service_info.instance_id).await {
                warn!(error = %e, "failed to unregister service");
            } else {
                info!("service unregistered successfully");
            }
        }

        // 停止 gRPC 服务器
        grpc_server_handle.abort();

        // 停止长连接服务器
        if let Some(mut server) = context.long_connection_server.lock().await.take() {
            if let Err(e) = server.stop().await {
                warn!(error = %e, "failed to stop long connection server");
            }
        }
    }
}

