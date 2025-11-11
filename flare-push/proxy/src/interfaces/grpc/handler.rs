use std::sync::Arc;

use flare_proto::push::{
    PushMessageRequest, PushMessageResponse, PushNotificationRequest, PushNotificationResponse,
};
use tonic::{Request, Response, Status};
use tracing::error;

use crate::application::commands::PushCommandService;

pub struct PushGrpcHandler {
    command_service: Arc<PushCommandService>,
}

impl PushGrpcHandler {
    pub fn new(command_service: Arc<PushCommandService>) -> Self {
        Self { command_service }
    }

    pub async fn push_message(
        &self,
        request: Request<PushMessageRequest>,
    ) -> Result<Response<PushMessageResponse>, Status> {
        let req = request.into_inner();
        match self.command_service.enqueue_message(req).await {
            Ok(resp) => Ok(Response::new(resp)),
            Err(err) => {
                error!(?err, "failed to enqueue push message");
                Err(Status::internal(err.to_string()))
            }
        }
    }

    pub async fn push_notification(
        &self,
        request: Request<PushNotificationRequest>,
    ) -> Result<Response<PushNotificationResponse>, Status> {
        let req = request.into_inner();
        match self.command_service.enqueue_notification(req).await {
            Ok(resp) => Ok(Response::new(resp)),
            Err(err) => {
                error!(?err, "failed to enqueue push notification");
                Err(Status::internal(err.to_string()))
            }
        }
    }
}
