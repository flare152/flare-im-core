use std::sync::Arc;

use flare_proto::push::push_service_server::PushService;
use flare_proto::push::{
    CancelScheduledPushRequest, CancelScheduledPushResponse, CreateTemplateRequest,
    CreateTemplateResponse, DeleteTemplateRequest, DeleteTemplateResponse,
    ListTemplatesRequest, ListTemplatesResponse, PushMessageRequest, PushMessageResponse,
    PushNotificationRequest, PushNotificationResponse, QueryPushStatusRequest,
    QueryPushStatusResponse, SchedulePushRequest, SchedulePushResponse,
    UpdateTemplateRequest, UpdateTemplateResponse,
};
use tonic::{Request, Response, Status};
use tracing::{error, info};

use crate::application::commands::{EnqueueMessageCommand, EnqueueNotificationCommand};
use crate::application::handlers::PushCommandHandler;

#[derive(Clone)]
pub struct PushGrpcHandler {
    command_handler: Arc<PushCommandHandler>,
}

impl PushGrpcHandler {
    pub fn new(command_handler: Arc<PushCommandHandler>) -> Self {
        Self { command_handler }
    }

    pub async fn push_message(
        &self,
        request: Request<PushMessageRequest>,
    ) -> Result<Response<PushMessageResponse>, Status> {
        let req = request.into_inner();
        let command = EnqueueMessageCommand { request: req };
        match self.command_handler.handle_enqueue_message(command).await {
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
        let command = EnqueueNotificationCommand { request: req };
        match self.command_handler.handle_enqueue_notification(command).await {
            Ok(resp) => Ok(Response::new(resp)),
            Err(err) => {
                error!(?err, "failed to enqueue push notification");
                Err(Status::internal(err.to_string()))
            }
        }
    }
}

#[tonic::async_trait]
impl PushService for PushGrpcHandler {
    async fn push_message(
        &self,
        request: Request<PushMessageRequest>,
    ) -> Result<Response<PushMessageResponse>, Status> {
        info!(
            "Push message request: {} users",
            request.get_ref().user_ids.len()
        );
        self.push_message(request).await
    }

    async fn push_notification(
        &self,
        request: Request<PushNotificationRequest>,
    ) -> Result<Response<PushNotificationResponse>, Status> {
        info!(
            "Push notification request: {} users",
            request.get_ref().user_ids.len()
        );
        self.push_notification(request).await
    }

    async fn create_template(
        &self,
        _request: Request<CreateTemplateRequest>,
    ) -> Result<Response<CreateTemplateResponse>, Status> {
        Err(Status::unimplemented("create_template not implemented yet"))
    }

    async fn update_template(
        &self,
        _request: Request<UpdateTemplateRequest>,
    ) -> Result<Response<UpdateTemplateResponse>, Status> {
        Err(Status::unimplemented("update_template not implemented yet"))
    }

    async fn delete_template(
        &self,
        _request: Request<DeleteTemplateRequest>,
    ) -> Result<Response<DeleteTemplateResponse>, Status> {
        Err(Status::unimplemented("delete_template not implemented yet"))
    }

    async fn list_templates(
        &self,
        _request: Request<ListTemplatesRequest>,
    ) -> Result<Response<ListTemplatesResponse>, Status> {
        Err(Status::unimplemented("list_templates not implemented yet"))
    }

    async fn schedule_push(
        &self,
        _request: Request<SchedulePushRequest>,
    ) -> Result<Response<SchedulePushResponse>, Status> {
        Err(Status::unimplemented("schedule_push not implemented yet"))
    }

    async fn cancel_scheduled_push(
        &self,
        _request: Request<CancelScheduledPushRequest>,
    ) -> Result<Response<CancelScheduledPushResponse>, Status> {
        Err(Status::unimplemented("cancel_scheduled_push not implemented yet"))
    }

    async fn query_push_status(
        &self,
        _request: Request<QueryPushStatusRequest>,
    ) -> Result<Response<QueryPushStatusResponse>, Status> {
        Err(Status::unimplemented("query_push_status not implemented yet"))
    }
}
