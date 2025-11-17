use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::info;

use crate::application::service::SessionApplicationService;
use crate::config::SessionConfig;
use crate::domain::repositories::MessageProvider;
use crate::infrastructure::persistence::redis_presence::RedisPresenceRepository;
use crate::infrastructure::persistence::redis_repository::RedisSessionRepository;
use crate::infrastructure::persistence::PostgresSessionRepository;
use crate::infrastructure::transport::storage_reader::StorageReaderMessageProvider;
use crate::interface::grpc::handler::SessionGrpcHandler;
use crate::interface::grpc::server::GrpcServer;

pub struct SessionServiceApp {
    grpc_server: GrpcServer,
}

impl SessionServiceApp {
    pub async fn new() -> Result<Self> {
        use flare_im_core::{load_config, ServiceHelper};
        
        // 加载应用配置
        let app_config = load_config(Some("config"));
        let service_config = app_config.session_service();
        
        // 组合运行时配置（保留以备将来使用）
        let _runtime_config = app_config.compose_service_config(
            &service_config.runtime,
            "flare-session",
        );
        
        let address: SocketAddr = ServiceHelper::parse_server_addr(
            app_config,
            &service_config.runtime,
            "flare-session",
        )
        .context("invalid session server address")?;

        let session_config = Arc::new(
            SessionConfig::from_app_config(app_config)
                .context("Failed to load session service configuration")?,
        );

        let redis_client = Arc::new(redis::Client::open(session_config.redis_url.clone())?);

        // 根据配置选择使用 PostgreSQL 或 Redis 仓库
        // PostgreSQL 用于会话元数据（创建、更新、删除、查询）
        // Redis 用于状态和光标（bootstrap、cursor更新）
        let session_repo: Arc<dyn crate::domain::repositories::SessionRepository> = if let Some(ref postgres_url) = session_config.postgres_url {
            info!(postgres_url = %postgres_url, "Using PostgreSQL repository for session metadata");
            // 使用 PostgreSQL 仓库（支持所有操作）
            let pool = Arc::new(
                sqlx::PgPool::connect(postgres_url)
                    .await
                    .context("Failed to connect to PostgreSQL")?,
            );
            Arc::new(PostgresSessionRepository::new(pool, session_config.clone()))
        } else {
            info!("PostgreSQL URL not configured, using Redis repository (limited functionality)");
            // 使用 Redis 仓库（仅支持状态和光标操作）
            Arc::new(RedisSessionRepository::new(
                redis_client.clone(),
                session_config.clone(),
            ))
        };
        let presence_repo = Arc::new(RedisPresenceRepository::new(
            redis_client.clone(),
            session_config.clone(),
        )) as Arc<_>;

        let message_provider: Option<Arc<dyn MessageProvider>> = session_config
            .storage_reader_endpoint
            .clone()
            .map(StorageReaderMessageProvider::new)
            .map(|provider| Arc::new(provider) as Arc<dyn MessageProvider>);

        let application_service = Arc::new(SessionApplicationService::new(
            session_repo,
            presence_repo,
            message_provider,
            session_config.clone(),
        ));

        let handler = SessionGrpcHandler::new(application_service);

        let grpc_server = GrpcServer::new(handler, address);

        Ok(Self { grpc_server })
    }

    pub async fn run(&self) -> Result<()> {
        info!(address = %self.grpc_server.address(), "flare-session listening");
        self.grpc_server.run().await
    }

    pub fn address(&self) -> SocketAddr {
        self.grpc_server.address()
    }
}

