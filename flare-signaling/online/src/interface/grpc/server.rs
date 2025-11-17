use std::sync::Arc;

use flare_proto::signaling::signaling_service_server::SignalingService;
use flare_proto::signaling::*;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use tracing::info;

use crate::interface::grpc::handler::OnlineHandler;

#[derive(Clone)]
pub struct SignalingOnlineServer {
    handler: Arc<OnlineHandler>,
}

impl SignalingOnlineServer {
    /// 从已有的 handler 创建服务器（用于 bootstrap）
    pub fn from_handler(handler: Arc<OnlineHandler>) -> Self {
        Self { handler }
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

    async fn subscribe(
        &self,
        request: Request<SubscribeRequest>,
    ) -> std::result::Result<Response<SubscribeResponse>, Status> {
        self.handler.handle_subscribe(request).await
    }

    async fn unsubscribe(
        &self,
        request: Request<UnsubscribeRequest>,
    ) -> std::result::Result<Response<UnsubscribeResponse>, Status> {
        self.handler.handle_unsubscribe(request).await
    }

    async fn publish_signal(
        &self,
        request: Request<PublishSignalRequest>,
    ) -> std::result::Result<Response<PublishSignalResponse>, Status> {
        self.handler.handle_publish_signal(request).await
    }

    type WatchPresenceStream = ReceiverStream<std::result::Result<PresenceEvent, Status>>;

    async fn watch_presence(
        &self,
        request: Request<WatchPresenceRequest>,
    ) -> std::result::Result<Response<Self::WatchPresenceStream>, Status> {
        self.handler.handle_watch_presence(request).await
    }
}
