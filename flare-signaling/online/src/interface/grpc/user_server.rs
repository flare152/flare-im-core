//! UserService gRPC 服务器实现

use std::sync::Arc;

use flare_proto::signaling::user_service_server::UserService;
use flare_proto::signaling::*;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

use crate::interface::grpc::user_handler::UserHandler;

#[derive(Clone)]
pub struct UserServiceServer {
    handler: Arc<UserHandler>,
}

impl UserServiceServer {
    pub fn new(handler: Arc<UserHandler>) -> Self {
        Self { handler }
    }
}

#[tonic::async_trait]
impl UserService for UserServiceServer {
    async fn get_user_presence(
        &self,
        request: Request<GetUserPresenceRequest>,
    ) -> std::result::Result<Response<GetUserPresenceResponse>, Status> {
        self.handler.handle_get_user_presence(request).await
    }

    async fn batch_get_user_presence(
        &self,
        request: Request<BatchGetUserPresenceRequest>,
    ) -> std::result::Result<Response<BatchGetUserPresenceResponse>, Status> {
        self.handler.handle_batch_get_user_presence(request).await
    }

    type SubscribeUserPresenceStream = ReceiverStream<std::result::Result<UserPresenceEvent, Status>>;

    async fn subscribe_user_presence(
        &self,
        request: Request<SubscribeUserPresenceRequest>,
    ) -> std::result::Result<Response<Self::SubscribeUserPresenceStream>, Status> {
        self.handler.handle_subscribe_user_presence(request).await
    }

    async fn list_user_devices(
        &self,
        request: Request<ListUserDevicesRequest>,
    ) -> std::result::Result<Response<ListUserDevicesResponse>, Status> {
        self.handler.handle_list_user_devices(request).await
    }

    async fn kick_device(
        &self,
        request: Request<KickDeviceRequest>,
    ) -> std::result::Result<Response<KickDeviceResponse>, Status> {
        self.handler.handle_kick_device(request).await
    }

    async fn get_device(
        &self,
        request: Request<GetDeviceRequest>,
    ) -> std::result::Result<Response<GetDeviceResponse>, Status> {
        self.handler.handle_get_device(request).await
    }
}

