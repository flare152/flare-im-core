//! AccessGateway gRPC 服务实现
//!
//! 提供业务系统推送消息给客户端的 gRPC 接口

use std::sync::Arc;

use crate::application::commands::{
    BatchPushMessageCommand, PushMessageCommand, PushMessageService,
};
use crate::application::queries::{ConnectionQueryService, QueryUserConnectionsQuery};
use flare_proto::access_gateway::access_gateway_server::AccessGateway;
use flare_proto::access_gateway::{
    BatchPushMessageRequest, BatchPushMessageResponse, PushMessageRequest, PushMessageResponse,
    QueryUserConnectionsRequest, QueryUserConnectionsResponse,
};
use async_trait::async_trait;
use tonic::{Request, Response, Status};
use tracing::info;

/// AccessGateway gRPC 服务实现
#[derive(Clone)]
pub struct AccessGatewayServer {
    push_service: Arc<PushMessageService>,
    connection_query_service: Arc<ConnectionQueryService>,
}

impl AccessGatewayServer {
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
impl AccessGateway for AccessGatewayServer {
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

