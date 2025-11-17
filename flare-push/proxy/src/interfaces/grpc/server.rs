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
