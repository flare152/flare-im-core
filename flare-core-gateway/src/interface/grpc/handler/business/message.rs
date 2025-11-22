//! # MessageService Handler
//!
//! 提供消息发送、查询、撤回等核心能力，委托给Message Orchestrator。

use std::sync::Arc;

use anyhow::Result;
// TODO: 等待business.proto生成Rust代码后启用
// use flare_proto::business::message_service_server::MessageService;
// use flare_proto::business::{
//     BatchSendMessageRequest, BatchSendMessageResponse, GetMessageRequest, GetMessageResponse,
//     QueryMessagesRequest, QueryMessagesRequest as BusinessQueryMessagesRequest,
//     QueryMessagesResponse, RecallMessageRequest, RecallMessageResponse, SearchMessagesRequest,
//     SearchMessagesResponse, SendMessageRequest, SendMessageResponse,
// };
use flare_proto::storage::{
    QueryMessagesRequest as StorageQueryMessagesRequest, QueryMessagesResponse as StorageQueryMessagesResponse,
};
use tonic::{Request, Response, Status};
use tracing::{debug, info};

use crate::infrastructure::storage::StorageClient;
use crate::interface::interceptor::{extract_claims, extract_tenant_context};

/// MessageService Handler实现
#[allow(dead_code)]
pub struct MessageServiceHandler {
    // Storage客户端（用于查询）
    storage_client: Arc<dyn StorageClient>,
    // Message Orchestrator客户端（TODO: 需要实现）
    // message_orchestrator_client: Arc<dyn MessageOrchestratorClient>,
}

impl MessageServiceHandler {
    /// 创建MessageService Handler
    pub fn new(storage_client: Arc<dyn StorageClient>) -> Self {
        Self {
            storage_client,
            // message_orchestrator_client,
        }
    }
}

// TODO: 等待business.proto生成Rust代码后启用
/*
#[tonic::async_trait]
impl MessageService for MessageServiceHandler {
    /// 发送单条消息
    async fn send_message(
        &self,
        request: Request<SendMessageRequest>,
    ) -> Result<Response<SendMessageResponse>, Status> {
        let req = request.into_inner();
        let claims = extract_claims(&request)
            .ok_or_else(|| Status::unauthenticated("Missing authentication"))?;
        let tenant_context = extract_tenant_context(&request)
            .ok_or_else(|| Status::invalid_argument("Missing tenant context"))?;

        info!(
            tenant_id = %tenant_context.tenant_id,
            actor_id = %claims.actor_id,
            "SendMessage request"
        );

        // TODO: 委托给Message Orchestrator
        // 1. 构建MessageDraft
        // 2. 调用Hook引擎PreSend Hook
        // 3. 调用Message Orchestrator发送消息
        // 4. 调用Hook引擎PostSend Hook

        Err(Status::unimplemented("SendMessage not yet implemented"))
    }

    /// 批量发送消息
    async fn batch_send_message(
        &self,
        request: Request<BatchSendMessageRequest>,
    ) -> Result<Response<BatchSendMessageResponse>, Status> {
        let _req = request.into_inner();
        debug!("BatchSendMessage request");

        // TODO: 实现批量发送逻辑
        Err(Status::unimplemented("BatchSendMessage not yet implemented"))
    }

    /// 查询消息
    async fn query_messages(
        &self,
        request: Request<QueryMessagesRequest>,
    ) -> Result<Response<QueryMessagesResponse>, Status> {
        let req = request.into_inner();
        let tenant_context = extract_tenant_context(&request)
            .ok_or_else(|| Status::invalid_argument("Missing tenant context"))?;

        debug!(
            tenant_id = %tenant_context.tenant_id,
            session_id = %req.session_id,
            "QueryMessages request"
        );

        // 转换为Storage QueryMessagesRequest
        let storage_req = StorageQueryMessagesRequest {
            session_id: req.session_id,
            cursor: req.cursor,
            limit: req.limit,
            direction: req.direction,
            filters: req.filters,
            sort: req.sort,
            context: req.context,
            tenant: Some(tenant_context.clone()),
        };

        // 调用Storage客户端查询消息
        match self.storage_client.query_messages(storage_req).await {
            Ok(storage_resp) => {
                // 转换为Business QueryMessagesResponse
                let business_resp = QueryMessagesResponse {
                    messages: storage_resp.messages,
                    cursor: storage_resp.cursor,
                    has_more: storage_resp.has_more,
                    total: storage_resp.total,
                    status: storage_resp.status,
                };
                Ok(Response::new(business_resp))
            }
            Err(e) => Err(Status::internal(format!("Query messages failed: {}", e))),
        }
    }

    /// 搜索消息
    async fn search_messages(
        &self,
        request: Request<SearchMessagesRequest>,
    ) -> Result<Response<SearchMessagesResponse>, Status> {
        let _req = request.into_inner();
        debug!("SearchMessages request");

        // TODO: 实现搜索逻辑
        Err(Status::unimplemented("SearchMessages not yet implemented"))
    }

    /// 获取消息详情
    async fn get_message(
        &self,
        request: Request<GetMessageRequest>,
    ) -> Result<Response<GetMessageResponse>, Status> {
        let _req = request.into_inner();
        debug!("GetMessage request");

        // TODO: 实现获取消息详情逻辑
        Err(Status::unimplemented("GetMessage not yet implemented"))
    }

    /// 撤回消息
    async fn recall_message(
        &self,
        request: Request<RecallMessageRequest>,
    ) -> Result<Response<RecallMessageResponse>, Status> {
        let _req = request.into_inner();
        debug!("RecallMessage request");

        // TODO: 实现撤回逻辑
        Err(Status::unimplemented("RecallMessage not yet implemented"))
    }
}
*/

