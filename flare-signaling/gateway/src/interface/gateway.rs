//! 统一网关处理器
//!
//! 同时处理 gRPC 接口和长连接

use std::sync::Arc;

use crate::application::handlers::{GetOnlineStatusQuery, SessionQueryService};
use crate::application::handlers::{
    HeartbeatCommand, LoginCommand, LogoutCommand, SessionCommandService,
};
use crate::interface::connection::LongConnectionHandler;
use flare_core::server::connection::ConnectionManagerTrait;
use flare_core::server::handle::ServerHandle;
use flare_proto::signaling::*;
use tonic::{Request, Response, Status};
use tracing::info;

pub struct UnifiedGatewayHandler {
    command_service: Arc<SessionCommandService>,
    query_service: Arc<SessionQueryService>,
    connection_handler: Arc<LongConnectionHandler>,
}

impl UnifiedGatewayHandler {
    pub fn new(
        command_service: Arc<SessionCommandService>,
        query_service: Arc<SessionQueryService>,
        connection_handler: Arc<LongConnectionHandler>,
    ) -> Self {
        Self {
            command_service,
            query_service,
            connection_handler,
        }
    }

    pub async fn set_server_handle(&self, handle: Arc<dyn ServerHandle>) {
        self.connection_handler.set_server_handle(handle).await;
    }

    pub async fn set_connection_manager(&self, manager: Arc<dyn ConnectionManagerTrait>) {
        self.connection_handler
            .set_connection_manager(manager)
            .await;
    }

    pub fn connection_handler(&self) -> Arc<dyn flare_core::server::ConnectionHandler> {
        self.connection_handler.clone()
    }

    pub async fn handle_login(
        &self,
        request: Request<LoginRequest>,
    ) -> Result<Response<LoginResponse>, Status> {
        let resp = self
            .command_service
            .handle_login(LoginCommand {
                request: request.into_inner(),
            })
            .await
            .map_err(Status::from)?;
        info!(session_id = %resp.session_id, "user login processed");
        Ok(Response::new(resp))
    }

    pub async fn handle_logout(
        &self,
        request: Request<LogoutRequest>,
    ) -> Result<Response<LogoutResponse>, Status> {
        let req = request.into_inner();
        let (response, removed_session) = self
            .command_service
            .handle_logout(LogoutCommand {
                request: req.clone(),
            })
            .await
            .map_err(Status::from)?;

        if let Some(session) = removed_session {
            if let Some(connection_id) = session.connection_id {
                self.connection_handler
                    .disconnect_connection(&connection_id)
                    .await;
            }
        }

        Ok(Response::new(response))
    }

    pub async fn handle_heartbeat(
        &self,
        request: Request<HeartbeatRequest>,
    ) -> Result<Response<HeartbeatResponse>, Status> {
        let response = self
            .command_service
            .handle_heartbeat(HeartbeatCommand {
                request: request.into_inner(),
            })
            .await
            .map_err(Status::from)?;
        Ok(Response::new(response))
    }

    pub async fn handle_get_online_status(
        &self,
        request: Request<GetOnlineStatusRequest>,
    ) -> Result<Response<GetOnlineStatusResponse>, Status> {
        let response = self
            .query_service
            .handle_get_online_status(GetOnlineStatusQuery {
                request: request.into_inner(),
            })
            .await
            .map_err(Status::from)?;
        Ok(Response::new(response))
    }
}
