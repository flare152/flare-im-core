use std::collections::HashMap;
use std::sync::Arc;

use flare_proto::signaling::signaling_service_server::SignalingService;
use flare_proto::signaling::*;
use flare_server_core::Config;
use flare_server_core::error::{ErrorCode, Result};
use tonic::{Request, Response, Status};
use tracing::info;

use crate::application::RouteDirectoryService;
use crate::config::RouteConfig;
use crate::infrastructure::persistence::memory::InMemoryRouteRepository;
use crate::interface::grpc::handler::RouteHandler;
use crate::util;

pub struct SignalingRouteServer {
    handler: Arc<RouteHandler>,
}

impl SignalingRouteServer {
    pub async fn new(_config: Config) -> Result<Self> {
        let route_config = Arc::new(RouteConfig::from_env());
        let repository = Arc::new(InMemoryRouteRepository::new(
            route_config.default_services.clone(),
        ));
        let service = RouteDirectoryService::new(repository, &route_config);
        for (svid, endpoint) in &route_config.default_services {
            service.register(svid.clone(), endpoint.clone()).await?;
        }
        let service = Arc::new(service);
        let handler = Arc::new(RouteHandler::new(service));
        Ok(Self { handler })
    }
}

#[tonic::async_trait]
impl SignalingService for SignalingRouteServer {
    async fn login(
        &self,
        _request: Request<LoginRequest>,
    ) -> std::result::Result<Response<LoginResponse>, Status> {
        Ok(Response::new(LoginResponse {
            success: false,
            session_id: String::new(),
            route_server: String::new(),
            error_message: "login not supported".to_string(),
            status: util::rpc_status_error(ErrorCode::OperationNotSupported, "login not supported"),
        }))
    }

    async fn logout(
        &self,
        _request: Request<LogoutRequest>,
    ) -> std::result::Result<Response<LogoutResponse>, Status> {
        Ok(Response::new(LogoutResponse {
            success: false,
            status: util::rpc_status_error(
                ErrorCode::OperationNotSupported,
                "logout not supported",
            ),
        }))
    }

    async fn heartbeat(
        &self,
        _request: Request<HeartbeatRequest>,
    ) -> std::result::Result<Response<HeartbeatResponse>, Status> {
        Ok(Response::new(HeartbeatResponse {
            success: false,
            status: util::rpc_status_error(
                ErrorCode::OperationNotSupported,
                "heartbeat not supported",
            ),
        }))
    }

    async fn get_online_status(
        &self,
        _request: Request<GetOnlineStatusRequest>,
    ) -> std::result::Result<Response<GetOnlineStatusResponse>, Status> {
        Ok(Response::new(GetOnlineStatusResponse {
            statuses: HashMap::new(),
            status: util::rpc_status_error(
                ErrorCode::OperationNotSupported,
                "online status not supported",
            ),
        }))
    }

    async fn route_message(
        &self,
        request: Request<RouteMessageRequest>,
    ) -> std::result::Result<Response<RouteMessageResponse>, Status> {
        info!(user_id = %request.get_ref().user_id, svid = %request.get_ref().svid, "Route message request");
        self.handler.handle_route_message(request).await
    }
}
