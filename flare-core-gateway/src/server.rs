use std::sync::Arc;

use anyhow::Result;
use flare_proto::communication_core::communication_core_server::CommunicationCore;
use flare_proto::communication_core::*;
use flare_proto::TenantContext;
use tonic::{Request, Response, Status};

use crate::config::GatewayConfig;
use crate::handler::{gateway::tenant_id_label, GatewayHandler};
use crate::infrastructure::{GrpcPushClient, GrpcSignalingClient, GrpcStorageClient};
use tracing::debug;

pub struct CommunicationCoreGatewayServer {
    handler: Arc<GatewayHandler>,
}

impl CommunicationCoreGatewayServer {
    pub async fn new(config: GatewayConfig) -> Result<Self> {
        let signaling = GrpcSignalingClient::new(config.signaling_endpoint.clone());
        let storage = GrpcStorageClient::new(config.message_endpoint.clone());
        let push = GrpcPushClient::new(config.push_endpoint.clone());

        let handler = Arc::new(GatewayHandler::new(signaling, storage, push));
        Ok(Self { handler })
    }
}

fn request_span<'a>(
    method: &'static str,
    tenant: Option<&'a TenantContext>,
) -> tracing::Span {
    let tenant_label = tenant_id_label(tenant);
    tracing::info_span!(method, tenant = tenant_label)
}

#[tonic::async_trait]
impl CommunicationCore for CommunicationCoreGatewayServer {
    async fn login(
        &self,
        request: Request<LoginRequest>,
    ) -> Result<Response<LoginResponse>, Status> {
        let span = request_span("communication_core.login", request.get_ref().tenant.as_ref());
        let _guard = span.enter();
        debug!("Gateway login request");
        let response = self
            .handler
            .handle_login(request.into_inner())
            .await
            .map_err(Status::from)?;
        Ok(Response::new(response))
    }

    async fn get_online_status(
        &self,
        request: Request<GetOnlineStatusRequest>,
    ) -> Result<Response<GetOnlineStatusResponse>, Status> {
        let span = request_span(
            "communication_core.get_online_status",
            request.get_ref().tenant.as_ref(),
        );
        let _guard = span.enter();
        let response = self
            .handler
            .handle_get_online_status(request.into_inner())
            .await
            .map_err(Status::from)?;
        Ok(Response::new(response))
    }

    async fn route_message(
        &self,
        request: Request<RouteMessageRequest>,
    ) -> Result<Response<RouteMessageResponse>, Status> {
        let span =
            request_span("communication_core.route_message", request.get_ref().tenant.as_ref());
        let _guard = span.enter();
        let response = self
            .handler
            .handle_route_message(request.into_inner())
            .await
            .map_err(Status::from)?;
        Ok(Response::new(response))
    }

    async fn store_message(
        &self,
        request: Request<StoreMessageRequest>,
    ) -> Result<Response<StoreMessageResponse>, Status> {
        let span =
            request_span("communication_core.store_message", request.get_ref().tenant.as_ref());
        let _guard = span.enter();
        let response = self
            .handler
            .handle_store_message(request.into_inner())
            .await
            .map_err(Status::from)?;
        Ok(Response::new(response))
    }

    async fn query_messages(
        &self,
        request: Request<QueryMessagesRequest>,
    ) -> Result<Response<QueryMessagesResponse>, Status> {
        let span =
            request_span("communication_core.query_messages", request.get_ref().tenant.as_ref());
        let _guard = span.enter();
        let response = self
            .handler
            .handle_query_messages(request.into_inner())
            .await
            .map_err(Status::from)?;
        Ok(Response::new(response))
    }

    async fn push_message(
        &self,
        request: Request<PushMessageRequest>,
    ) -> Result<Response<PushMessageResponse>, Status> {
        let span =
            request_span("communication_core.push_message", request.get_ref().tenant.as_ref());
        let _guard = span.enter();
        let response = self
            .handler
            .handle_push_message(request.into_inner())
            .await
            .map_err(Status::from)?;
        Ok(Response::new(response))
    }

    async fn push_notification(
        &self,
        request: Request<PushNotificationRequest>,
    ) -> Result<Response<PushNotificationResponse>, Status> {
        let span = request_span(
            "communication_core.push_notification",
            request.get_ref().tenant.as_ref(),
        );
        let _guard = span.enter();
        let response = self
            .handler
            .handle_push_notification(request.into_inner())
            .await
            .map_err(Status::from)?;
        Ok(Response::new(response))
    }

    async fn upload_file(
        &self,
        _request: Request<tonic::Streaming<UploadFileRequest>>,
    ) -> Result<Response<UploadFileResponse>, Status> {
        Err(Status::unimplemented("media upload not yet supported"))
    }

    async fn get_file_url(
        &self,
        _request: Request<GetFileUrlRequest>,
    ) -> Result<Response<GetFileUrlResponse>, Status> {
        Err(Status::unimplemented("media operations not yet supported"))
    }

    async fn get_file_info(
        &self,
        _request: Request<GetFileInfoRequest>,
    ) -> Result<Response<GetFileInfoResponse>, Status> {
        Err(Status::unimplemented("media operations not yet supported"))
    }

    async fn delete_file(
        &self,
        _request: Request<DeleteFileRequest>,
    ) -> Result<Response<DeleteFileResponse>, Status> {
        Err(Status::unimplemented("media operations not yet supported"))
    }

    async fn process_image(
        &self,
        _request: Request<ProcessImageRequest>,
    ) -> Result<Response<ProcessImageResponse>, Status> {
        Err(Status::unimplemented("media operations not yet supported"))
    }

    async fn process_video(
        &self,
        _request: Request<ProcessVideoRequest>,
    ) -> Result<Response<ProcessVideoResponse>, Status> {
        Err(Status::unimplemented("media operations not yet supported"))
    }
}

