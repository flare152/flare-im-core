use crate::handler::StorageReaderHandler;
use flare_proto::storage::storage_service_server::StorageService;
use flare_proto::storage::*;
use flare_server_core::{Config, error};
use std::sync::Arc;
use tonic::{Request, Response, Status};
use tracing::info;

pub struct StorageReaderServer {
    handler: Arc<StorageReaderHandler>,
}

impl StorageReaderServer {
    pub async fn new(config: Config) -> anyhow::Result<Self> {
        let handler = Arc::new(StorageReaderHandler::new().await?);

        info!(
            "storage-reader starting on {}:{}",
            config.server.address, config.server.port
        );

        Ok(Self { handler })
    }
}

#[tonic::async_trait]
impl StorageService for StorageReaderServer {
    async fn store_message(
        &self,
        _request: Request<StoreMessageRequest>,
    ) -> Result<Response<StoreMessageResponse>, Status> {
        Ok(Response::new(StoreMessageResponse {
            success: false,
            message_id: String::new(),
            error_message: "Store not supported in Reader service".to_string(),
            status: Some(error::ok_status()),
        }))
    }

    async fn batch_store_message(
        &self,
        _request: Request<BatchStoreMessageRequest>,
    ) -> Result<Response<BatchStoreMessageResponse>, Status> {
        Ok(Response::new(BatchStoreMessageResponse {
            success_count: 0,
            fail_count: 0,
            message_ids: vec![],
            failures: vec![],
            status: Some(error::ok_status()),
        }))
    }

    async fn query_messages(
        &self,
        request: Request<QueryMessagesRequest>,
    ) -> Result<Response<QueryMessagesResponse>, Status> {
        info!(
            "Query messages request: session_id={}",
            request.get_ref().session_id
        );
        self.handler.handle_query_messages(request).await
    }

    async fn delete_message(
        &self,
        request: Request<DeleteMessageRequest>,
    ) -> Result<Response<DeleteMessageResponse>, Status> {
        let mut response = self.handler.handle_delete_message(request).await?;
        response.get_mut().status = Some(error::ok_status());
        Ok(response)
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
        let mut response = self.handler.handle_delete_message_for_user(request).await?;
        response.get_mut().status = Some(error::ok_status());
        Ok(response)
    }

    async fn clear_session(
        &self,
        request: Request<ClearSessionRequest>,
    ) -> Result<Response<ClearSessionResponse>, Status> {
        let mut response = self.handler.handle_clear_session(request).await?;
        response.get_mut().status = Some(error::ok_status());
        Ok(response)
    }

    async fn mark_message_read(
        &self,
        request: Request<MarkMessageReadRequest>,
    ) -> Result<Response<MarkMessageReadResponse>, Status> {
        let mut response = self.handler.handle_mark_message_read(request).await?;
        response.get_mut().status = Some(error::ok_status());
        Ok(response)
    }

    // ========== 会话管理相关 ==========

    async fn create_session(
        &self,
        request: Request<CreateSessionRequest>,
    ) -> Result<Response<CreateSessionResponse>, Status> {
        let mut response = self.handler.handle_create_session(request).await?;
        response.get_mut().status = Some(error::ok_status());
        Ok(response)
    }

    async fn get_session(
        &self,
        request: Request<GetSessionRequest>,
    ) -> Result<Response<GetSessionResponse>, Status> {
        let mut response = self.handler.handle_get_session(request).await?;
        response.get_mut().status = Some(error::ok_status());
        Ok(response)
    }

    async fn update_session(
        &self,
        request: Request<UpdateSessionRequest>,
    ) -> Result<Response<UpdateSessionResponse>, Status> {
        let mut response = self.handler.handle_update_session(request).await?;
        response.get_mut().status = Some(error::ok_status());
        Ok(response)
    }

    async fn query_user_sessions(
        &self,
        request: Request<QueryUserSessionsRequest>,
    ) -> Result<Response<QueryUserSessionsResponse>, Status> {
        let mut response = self.handler.handle_query_user_sessions(request).await?;
        response.get_mut().status = Some(error::ok_status());
        Ok(response)
    }
}
