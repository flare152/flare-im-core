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
    QueryUserConnectionsRequest, QueryUserConnectionsResponse, PushAckRequest, PushCustomRequest,
};
use flare_proto::signaling::online::signaling_service_server::SignalingService;
use flare_proto::signaling::online::*;
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

    async fn push_ack(
        &self,
        request: Request<PushAckRequest>,
    ) -> Result<Response<PushMessageResponse>, Status> {
        let req = request.into_inner();
        let ack = req.ack.ok_or_else(|| Status::invalid_argument("ack is required"))?;
        
        info!(
            "PushAck request: {} users, message_id: {}",
            req.target_user_ids.len(),
            ack.message_id
        );
        
        // TODO: 实现 ACK 推送逻辑
        // 将 ACK 封装为 ServerPacket 推送给目标用户
        Ok(Response::new(PushMessageResponse {
            request_id: req.request_id,
            results: vec![],
            status: Some(flare_proto::RpcStatus {
                code: flare_proto::common::ErrorCode::Ok as i32,
                message: "ACK push not yet implemented".to_string(),
                details: vec![],
                context: None,
            }),
            statistics: None,
        }))
    }

    async fn push_custom(
        &self,
        request: Request<PushCustomRequest>,
    ) -> Result<Response<PushMessageResponse>, Status> {
        let req = request.into_inner();
        let custom = req.custom.ok_or_else(|| Status::invalid_argument("custom data is required"))?;
        
        info!(
            "PushCustom request: {} users, type: {}",
            req.target_user_ids.len(),
            custom.r#type
        );
        
        // TODO: 实现自定义数据推送逻辑
        // 将 CustomPushData 封装为 ServerPacket 推送给目标用户
        Ok(Response::new(PushMessageResponse {
            request_id: req.request_id,
            results: vec![],
            status: Some(flare_proto::RpcStatus {
                code: flare_proto::common::ErrorCode::Ok as i32,
                message: "Custom push not yet implemented".to_string(),
                details: vec![],
                context: None,
            }),
            statistics: None,
        }))
    }

    async fn subscribe(
        &self,
        request: Request<flare_proto::access_gateway::SubscribeRequest>,
    ) -> Result<Response<flare_proto::access_gateway::SubscribeResponse>, Status> {
        let req = request.into_inner();
        info!("Subscribe request: user_id={}", req.user_id);
        
        // TODO: 实现订阅逻辑
        Ok(Response::new(flare_proto::access_gateway::SubscribeResponse {
            granted: vec![],
            status: Some(flare_proto::RpcStatus {
                code: flare_proto::common::ErrorCode::Ok as i32,
                message: "Subscribe not yet implemented".to_string(),
                details: vec![],
                context: None,
            }),
        }))
    }

    async fn unsubscribe(
        &self,
        request: Request<flare_proto::access_gateway::UnsubscribeRequest>,
    ) -> Result<Response<flare_proto::access_gateway::UnsubscribeResponse>, Status> {
        let req = request.into_inner();
        info!("Unsubscribe request: user_id={}", req.user_id);
        
        // TODO: 实现取消订阅逻辑
        Ok(Response::new(flare_proto::access_gateway::UnsubscribeResponse {
            status: Some(flare_proto::RpcStatus {
                code: flare_proto::common::ErrorCode::Ok as i32,
                message: "Unsubscribe not yet implemented".to_string(),
                details: vec![],
                context: None,
            }),
        }))
    }

    async fn publish_signal(
        &self,
        request: Request<flare_proto::access_gateway::PublishSignalRequest>,
    ) -> Result<Response<flare_proto::access_gateway::PublishSignalResponse>, Status> {
        let _req = request.into_inner();
        info!("PublishSignal request received");
        
        // TODO: 实现发布信令逻辑
        Ok(Response::new(flare_proto::access_gateway::PublishSignalResponse {
            status: Some(flare_proto::RpcStatus {
                code: flare_proto::common::ErrorCode::Ok as i32,
                message: "PublishSignal not yet implemented".to_string(),
                details: vec![],
                context: None,
            }),
        }))
    }
}

