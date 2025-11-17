//! 统一网关 gRPC 服务器
//!
//! 对外暴露信令服务接口

use std::sync::Arc;

use crate::interface::gateway::UnifiedGatewayHandler;
use flare_core::server::connection::ConnectionManager;
use flare_proto::signaling::signaling_service_server::SignalingService;
use flare_proto::signaling::*;
use flare_server_core::Config;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use tracing::info;

/// 统一网关服务器
///
/// 提供 gRPC 服务接口，处理认证、会话管理等
#[derive(Clone)]
pub struct UnifiedGatewayServer {
    handler: Arc<UnifiedGatewayHandler>,
}

impl UnifiedGatewayServer {
    pub fn new(
        _config: Config,
        handler: Arc<UnifiedGatewayHandler>,
        _connection_manager: Arc<ConnectionManager>,
    ) -> Self {
        Self { handler }
    }
}

#[tonic::async_trait]
impl SignalingService for UnifiedGatewayServer {
    async fn login(
        &self,
        request: Request<LoginRequest>,
    ) -> Result<Response<LoginResponse>, Status> {
        info!("Login request: user_id={}", request.get_ref().user_id);
        self.handler.handle_login(request).await
    }

    async fn logout(
        &self,
        request: Request<LogoutRequest>,
    ) -> Result<Response<LogoutResponse>, Status> {
        info!("Logout request: user_id={}", request.get_ref().user_id);
        self.handler.handle_logout(request).await
    }

    async fn heartbeat(
        &self,
        request: Request<HeartbeatRequest>,
    ) -> Result<Response<HeartbeatResponse>, Status> {
        self.handler.handle_heartbeat(request).await
    }

    async fn get_online_status(
        &self,
        request: Request<GetOnlineStatusRequest>,
    ) -> Result<Response<GetOnlineStatusResponse>, Status> {
        self.handler.handle_get_online_status(request).await
    }

    async fn route_message(
        &self,
        request: Request<RouteMessageRequest>,
    ) -> Result<Response<RouteMessageResponse>, Status> {
        self.handler.handle_route_message(request).await
    }

    async fn subscribe(
        &self,
        _request: Request<SubscribeRequest>,
    ) -> Result<Response<SubscribeResponse>, Status> {
        Err(Status::unimplemented("Subscribe not implemented"))
    }

    async fn unsubscribe(
        &self,
        _request: Request<UnsubscribeRequest>,
    ) -> Result<Response<UnsubscribeResponse>, Status> {
        Err(Status::unimplemented("Unsubscribe not implemented"))
    }

    async fn publish_signal(
        &self,
        _request: Request<PublishSignalRequest>,
    ) -> Result<Response<PublishSignalResponse>, Status> {
        Err(Status::unimplemented("PublishSignal not implemented"))
    }

    type WatchPresenceStream = tokio_stream::wrappers::ReceiverStream<Result<PresenceEvent, Status>>;

    async fn watch_presence(
        &self,
        _request: Request<WatchPresenceRequest>,
    ) -> Result<Response<Self::WatchPresenceStream>, Status> {
        Err(Status::unimplemented("WatchPresence not implemented"))
    }
}
