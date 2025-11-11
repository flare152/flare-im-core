use std::sync::Arc;

use flare_proto::push::push_service_server::PushService;
use flare_proto::push::{
    PushMessageRequest, PushMessageResponse, PushNotificationRequest, PushNotificationResponse,
};
use tonic::{Request, Response, Status};
use tracing::info;

use crate::interfaces::grpc::handler::PushGrpcHandler;

#[derive(Clone)]
pub struct PushProxyGrpcServer {
    handler: Arc<PushGrpcHandler>,
}

impl PushProxyGrpcServer {
    pub fn new(handler: Arc<PushGrpcHandler>) -> Self {
        Self { handler }
    }
}

#[tonic::async_trait]
impl PushService for PushProxyGrpcServer {
    async fn push_message(
        &self,
        request: Request<PushMessageRequest>,
    ) -> Result<Response<PushMessageResponse>, Status> {
        info!(
            "Push message request: {} users",
            request.get_ref().user_ids.len()
        );
        self.handler.push_message(request).await
    }

    async fn push_notification(
        &self,
        request: Request<PushNotificationRequest>,
    ) -> Result<Response<PushNotificationResponse>, Status> {
        info!(
            "Push notification request: {} users",
            request.get_ref().user_ids.len()
        );
        self.handler.push_notification(request).await
    }
}
