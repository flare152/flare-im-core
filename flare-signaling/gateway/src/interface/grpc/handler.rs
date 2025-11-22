//! Access Gateway gRPC 服务处理器
//!
//! 直接实现 gRPC 服务 trait，无需 server 包装

use std::sync::Arc;

use crate::application::handlers::{
    BatchPushMessageCommand, PushMessageCommand, PushMessageService,
};
use crate::application::handlers::{ConnectionQueryService, QueryUserConnectionsQuery};
use crate::interface::gateway::UnifiedGatewayHandler;
use flare_proto::access_gateway::access_gateway_server::AccessGateway;
use flare_proto::access_gateway::{
    BatchPushMessageRequest, BatchPushMessageResponse, PushMessageRequest, PushMessageResponse,
    QueryUserConnectionsRequest, QueryUserConnectionsResponse,
};
use flare_proto::signaling::signaling_service_server::SignalingService;
use flare_proto::signaling::*;
use async_trait::async_trait;
use tonic::{Request, Response, Status};
use tracing::info;

/// UnifiedGateway gRPC 处理器
///
/// 提供信令服务 gRPC 接口，处理认证、会话管理等
#[derive(Clone)]
pub struct UnifiedGatewayGrpcHandler {
    handler: Arc<crate::interface::gateway::UnifiedGatewayHandler>,
}

impl UnifiedGatewayGrpcHandler {
    pub fn new(handler: Arc<crate::interface::gateway::UnifiedGatewayHandler>) -> Self {
        Self { handler }
    }
}

#[tonic::async_trait]
impl SignalingService for UnifiedGatewayGrpcHandler {
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

/// AccessGateway gRPC 处理器
///
/// 提供业务系统推送消息给客户端的 gRPC 接口
#[derive(Clone)]
pub struct AccessGatewayHandler {
    push_service: Arc<PushMessageService>,
    connection_query_service: Arc<ConnectionQueryService>,
}

impl AccessGatewayHandler {
    pub fn new(
        push_service: Arc<PushMessageService>,
        connection_query_service: Arc<ConnectionQueryService>,
    ) -> Self {
        Self {
            push_service,
            connection_query_service,
        }
    }
}

#[tonic::async_trait]
impl AccessGateway for AccessGatewayHandler {
    async fn push_message(
        &self,
        request: Request<PushMessageRequest>,
    ) -> Result<Response<PushMessageResponse>, Status> {
        let req = request.into_inner();
        info!(
            "PushMessage request: {} users",
            req.target_user_ids.len()
        );

        let response = self
            .push_service
            .handle_push_message(PushMessageCommand { request: req })
            .await
            .map_err(|e| {
                tracing::error!(?e, "Failed to push message");
                Status::internal(e.to_string())
            })?;

        Ok(Response::new(response))
    }

    async fn batch_push_message(
        &self,
        request: Request<BatchPushMessageRequest>,
    ) -> Result<Response<BatchPushMessageResponse>, Status> {
        let req = request.into_inner();
        info!(
            "BatchPushMessage request: {} tasks",
            req.pushes.len()
        );

        let response = self
            .push_service
            .handle_batch_push_message(BatchPushMessageCommand { request: req })
            .await
            .map_err(|e| {
                tracing::error!(?e, "Failed to batch push message");
                Status::internal(e.to_string())
            })?;

        Ok(Response::new(response))
    }

    async fn query_user_connections(
        &self,
        request: Request<QueryUserConnectionsRequest>,
    ) -> Result<Response<QueryUserConnectionsResponse>, Status> {
        let req = request.into_inner();
        info!("QueryUserConnections request: {} users", req.user_ids.len());

        let response = self
            .connection_query_service
            .handle_query_user_connections(QueryUserConnectionsQuery { request: req })
            .await
            .map_err(|e| {
                tracing::error!(?e, "Failed to query user connections");
                Status::internal(e.to_string())
            })?;

        Ok(Response::new(response))
    }
}

