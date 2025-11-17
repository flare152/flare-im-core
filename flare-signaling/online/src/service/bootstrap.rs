//! 应用启动器 - 负责依赖注入和服务启动
use crate::application::{OnlineStatusService, SubscriptionService, UserService};
use crate::config::OnlineConfig;
use crate::domain::repositories::{PresenceWatcher, SessionRepository, SignalPublisher, SubscriptionRepository};
use crate::infrastructure::persistence::redis::{
    RedisPresenceWatcher, RedisSessionRepository, RedisSignalPublisher,
    RedisSubscriptionRepository,
};
use crate::interface::grpc::{server::SignalingOnlineServer, user_server::UserServiceServer, user_handler::UserHandler};
use crate::service::registry::ServiceRegistrar;
use anyhow::{Context, Result};
use flare_im_core::FlareAppConfig;
use flare_server_core::ServiceRegistryTrait;
use redis::Client;
use std::sync::Arc;
use tokio::sync::RwLock;
use tonic::transport::Server;
use tracing::info;

/// 应用上下文 - 包含所有已初始化的服务
pub struct ApplicationContext {
    pub signaling_server: Arc<SignalingOnlineServer>,
    pub user_server: Arc<UserServiceServer>,
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
        let service_type = "signaling-online".to_string();

        // 注册服务
        let (service_registry, service_info) =
            ServiceRegistrar::register_service(&runtime_config, &service_type).await?;

        // 构建核心服务
        let online_config = Arc::new(
            OnlineConfig::from_app_config(config)
                .context("Failed to load online service configuration")?,
        );
        let redis_client = Arc::new(
            Client::open(online_config.redis_url.as_str())
                .map_err(|e| anyhow::anyhow!("Failed to create redis client: {}", e))?,
        );

        // 构建仓库
        let session_repository: Arc<dyn SessionRepository> =
            Arc::new(RedisSessionRepository::new(
                Arc::clone(&redis_client),
                online_config.clone(),
            ));

        let subscription_repository: Arc<dyn SubscriptionRepository> =
            Arc::new(RedisSubscriptionRepository::new(
                Arc::clone(&redis_client),
                online_config.clone(),
            ));

        let signal_publisher: Arc<dyn SignalPublisher> = Arc::new(RedisSignalPublisher::new(
            Arc::clone(&redis_client),
            online_config.clone(),
        ));

        let presence_watcher: Arc<dyn PresenceWatcher> = Arc::new(RedisPresenceWatcher::new(
            Arc::clone(&redis_client),
            online_config.clone(),
        ));

        // 构建应用服务
        let online_service = Arc::new(OnlineStatusService::new(
            online_config.clone(),
            session_repository.clone(),
        ));

        let subscription_service = Arc::new(SubscriptionService::new(
            subscription_repository,
            signal_publisher.clone(),
        ));

        // 构建 SignalingService 服务器
        use crate::interface::grpc::{handler::OnlineHandler, server::SignalingOnlineServer};
        let signaling_handler = Arc::new(OnlineHandler::new(
            online_service,
            subscription_service,
            presence_watcher.clone(),
        ));
        let signaling_server = Arc::new(SignalingOnlineServer::from_handler(signaling_handler));

        // 构建 UserService 服务器
        let user_service = Arc::new(UserService::new(session_repository.clone()));
        let user_handler = Arc::new(UserHandler::new(user_service, presence_watcher));
        let user_server = Arc::new(UserServiceServer::new(user_handler));

        Ok(ApplicationContext {
            signaling_server,
            user_server,
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
        info!(%addr, "starting signaling online service");

        let server_future = Server::builder()
            // 注册 SignalingService
            .add_service(
                flare_proto::signaling::signaling_service_server::SignalingServiceServer::new(
                    context.signaling_server.as_ref().clone(),
                ),
            )
            // 注册 UserService
            .add_service(
                flare_proto::signaling::user_service_server::UserServiceServer::new(
                    context.user_server.as_ref().clone(),
                ),
            )
            .serve(addr);

        // 等待服务器启动或接收到停止信号
        let result = tokio::select! {
            res = server_future => {
                if let Err(err) = res {
                    tracing::error!(error = %err, "signaling online service failed");
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

        info!("signaling online service stopped");
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

