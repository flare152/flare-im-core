use std::sync::Arc;

use flare_proto::signaling::*;
use flare_server_core::error::ErrorCode;
use tonic::{Request, Response, Status};
use tracing::error;

use crate::application::OnlineStatusService;
use crate::util;

pub struct OnlineHandler {
    service: Arc<OnlineStatusService>,
}

impl OnlineHandler {
    pub fn new(service: Arc<OnlineStatusService>) -> Self {
        Self { service }
    }

    pub async fn handle_login(
        &self,
        request: Request<LoginRequest>,
    ) -> std::result::Result<Response<LoginResponse>, Status> {
        match self.service.login(request.into_inner()).await {
            Ok(response) => Ok(Response::new(response)),
            Err(err) => {
                error!(?err, "login failed");
                Err(Status::from(err))
            }
        }
    }

    pub async fn handle_logout(
        &self,
        request: Request<LogoutRequest>,
    ) -> std::result::Result<Response<LogoutResponse>, Status> {
        match self.service.logout(request.into_inner()).await {
            Ok(response) => Ok(Response::new(response)),
            Err(err) => {
                error!(?err, "logout failed");
                Err(Status::from(err))
            }
        }
    }

    pub async fn handle_heartbeat(
        &self,
        request: Request<HeartbeatRequest>,
    ) -> std::result::Result<Response<HeartbeatResponse>, Status> {
        let req = request.into_inner();
        match self.service.heartbeat(&req.session_id, &req.user_id).await {
            Ok(response) => Ok(Response::new(response)),
            Err(err) => {
                error!(?err, "heartbeat failed");
                Err(Status::from(err))
            }
        }
    }

    pub async fn handle_get_online_status(
        &self,
        request: Request<GetOnlineStatusRequest>,
    ) -> std::result::Result<Response<GetOnlineStatusResponse>, Status> {
        let req = request.into_inner();
        match self.service.get_online_status(&req.user_ids).await {
            Ok(response) => Ok(Response::new(response)),
            Err(err) => {
                error!(?err, "get_online_status failed");
                Err(Status::from(err))
            }
        }
    }

    pub async fn handle_route_message(
        &self,
        _request: Request<RouteMessageRequest>,
    ) -> std::result::Result<Response<RouteMessageResponse>, Status> {
        Ok(Response::new(RouteMessageResponse {
            success: false,
            response: vec![],
            error_message: "Route not supported in Online service".to_string(),
            status: util::rpc_status_error(ErrorCode::InvalidParameter, "route not supported"),
        }))
    }
}
