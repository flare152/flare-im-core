use std::sync::Arc;

use flare_proto::signaling::signaling_service_server::SignalingService;
use flare_proto::signaling::*;
use flare_server_core::Config;
use flare_server_core::error::{ErrorBuilder, ErrorCode, Result};
use redis::Client;
use tonic::{Request, Response, Status};
use tracing::info;

use crate::application::OnlineStatusService;
use crate::config::OnlineConfig;
use crate::infrastructure::persistence::redis::RedisSessionRepository;
use crate::interface::grpc::handler::OnlineHandler;

pub struct SignalingOnlineServer {
    handler: Arc<OnlineHandler>,
}

impl SignalingOnlineServer {
    pub async fn new(_config: Config) -> Result<Self> {
        let online_config = Arc::new(OnlineConfig::from_env());
        let redis_client = Arc::new(Client::open(online_config.redis_url.as_str()).map_err(
            |err| {
                ErrorBuilder::new(
                    ErrorCode::ServiceUnavailable,
                    "failed to create redis client",
                )
                .details(err.to_string())
                .build_error()
            },
        )?);
        let repository = Arc::new(RedisSessionRepository::new(
            redis_client,
            online_config.clone(),
        ));
        let service = Arc::new(OnlineStatusService::new(online_config, repository));
        let handler = Arc::new(OnlineHandler::new(service));
        Ok(Self { handler })
    }
}

#[tonic::async_trait]
impl SignalingService for SignalingOnlineServer {
    async fn login(
        &self,
        request: Request<LoginRequest>,
    ) -> std::result::Result<Response<LoginResponse>, Status> {
        self.handler.handle_login(request).await
    }

    async fn logout(
        &self,
        request: Request<LogoutRequest>,
    ) -> std::result::Result<Response<LogoutResponse>, Status> {
        self.handler.handle_logout(request).await
    }

    async fn heartbeat(
        &self,
        request: Request<HeartbeatRequest>,
    ) -> std::result::Result<Response<HeartbeatResponse>, Status> {
        self.handler.handle_heartbeat(request).await
    }

    async fn get_online_status(
        &self,
        request: Request<GetOnlineStatusRequest>,
    ) -> std::result::Result<Response<GetOnlineStatusResponse>, Status> {
        info!("Get online status request");
        self.handler.handle_get_online_status(request).await
    }

    async fn route_message(
        &self,
        request: Request<RouteMessageRequest>,
    ) -> std::result::Result<Response<RouteMessageResponse>, Status> {
        self.handler.handle_route_message(request).await
    }
}
