use crate::handler::MessageOrchestratorHandler;
use crate::infrastructure::config::MessageOrchestratorConfig;
use anyhow::Result;
use flare_proto::storage::storage_service_server::StorageService;
use flare_proto::storage::*;
use std::sync::Arc;
use tonic::{Request, Response, Status};
use tracing::info;

pub struct MessageOrchestratorServer {
    handler: Arc<MessageOrchestratorHandler>,
}

impl MessageOrchestratorServer {
    pub async fn new(config: Arc<MessageOrchestratorConfig>) -> Result<Self> {
        let handler = Arc::new(MessageOrchestratorHandler::new(config).await?);
        Ok(Self { handler })
    }
}

#[tonic::async_trait]
impl StorageService for MessageOrchestratorServer {
    async fn store_message(
        &self,
        request: Request<StoreMessageRequest>,
    ) -> Result<Response<StoreMessageResponse>, Status> {
        info!(
            "Orchestrator received message: session_id={}",
            request.get_ref().session_id
        );
        self.handler.handle_store_message(request).await
    }

    async fn batch_store_message(
        &self,
        request: Request<BatchStoreMessageRequest>,
    ) -> Result<Response<BatchStoreMessageResponse>, Status> {
        info!(
            "Orchestrator received batch: {} messages",
            request.get_ref().messages.len()
        );
        self.handler.handle_batch_store_message(request).await
    }

    async fn query_messages(
        &self,
        request: Request<QueryMessagesRequest>,
    ) -> Result<Response<QueryMessagesResponse>, Status> {
        self.handler.handle_query_messages(request).await
    }

    async fn delete_message(
        &self,
        request: Request<DeleteMessageRequest>,
    ) -> Result<Response<DeleteMessageResponse>, Status> {
        self.handler.handle_delete_message(request).await
    }

    // ========== 消息操作相关 ==========

    async fn recall_message(
        &self,
        request: Request<RecallMessageRequest>,
    ) -> Result<Response<RecallMessageResponse>, Status> {
        self.handler.handle_recall_message(request).await
    }

    async fn delete_message_for_user(
        &self,
        request: Request<DeleteMessageForUserRequest>,
    ) -> Result<Response<DeleteMessageForUserResponse>, Status> {
        self.handler.handle_delete_message_for_user(request).await
    }

    async fn clear_session(
        &self,
        request: Request<ClearSessionRequest>,
    ) -> Result<Response<ClearSessionResponse>, Status> {
        self.handler.handle_clear_session(request).await
    }

    async fn mark_message_read(
        &self,
        request: Request<MarkMessageReadRequest>,
    ) -> Result<Response<MarkMessageReadResponse>, Status> {
        self.handler.handle_mark_message_read(request).await
    }

    // ========== 会话管理相关 ==========

    async fn create_session(
        &self,
        request: Request<CreateSessionRequest>,
    ) -> Result<Response<CreateSessionResponse>, Status> {
        self.handler.handle_create_session(request).await
    }

    async fn get_session(
        &self,
        request: Request<GetSessionRequest>,
    ) -> Result<Response<GetSessionResponse>, Status> {
        self.handler.handle_get_session(request).await
    }

    async fn update_session(
        &self,
        request: Request<UpdateSessionRequest>,
    ) -> Result<Response<UpdateSessionResponse>, Status> {
        self.handler.handle_update_session(request).await
    }

    async fn query_user_sessions(
        &self,
        request: Request<QueryUserSessionsRequest>,
    ) -> Result<Response<QueryUserSessionsResponse>, Status> {
        self.handler.handle_query_user_sessions(request).await
    }
}
