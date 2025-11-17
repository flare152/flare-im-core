use std::sync::Arc;

use anyhow::Context;
use flare_im_core::config::FlareAppConfig;
use flare_proto::storage::storage_reader_service_server::StorageReaderService;
use flare_proto::storage::*;
use flare_server_core::{Config, error};
use tonic::{Request, Response, Status};
use tracing::info;

use crate::interface::grpc::handler::StorageReaderHandler;

#[derive(Clone)]
pub struct StorageReaderServer {
    handler: Arc<StorageReaderHandler>,
    config: Config,
}

impl StorageReaderServer {
    pub async fn new(app_config: &FlareAppConfig, config: Config) -> anyhow::Result<Self> {
        let handler = Arc::new(StorageReaderHandler::new(app_config).await?);

        info!(
            "storage-reader starting on {}:{}",
            config.server.address, config.server.port
        );

        Ok(Self { handler, config })
    }

    pub fn config(&self) -> &Config {
        &self.config
    }
}

#[tonic::async_trait]
impl StorageReaderService for StorageReaderServer {
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

    // 注意：会话管理方法（create_session, get_session, update_session, query_user_sessions）
    // 不属于 StorageService，应该由 SessionService 处理
    // 这些方法已移除，避免混淆

    async fn search_messages(
        &self,
        request: Request<SearchMessagesRequest>,
    ) -> Result<Response<SearchMessagesResponse>, Status> {
        self.handler.handle_search_messages(request).await
    }

    async fn get_message(
        &self,
        request: Request<GetMessageRequest>,
    ) -> Result<Response<GetMessageResponse>, Status> {
        self.handler.handle_get_message(request).await
    }

    async fn set_message_attributes(
        &self,
        request: Request<SetMessageAttributesRequest>,
    ) -> Result<Response<SetMessageAttributesResponse>, Status> {
        self.handler.handle_set_message_attributes(request).await
    }

    async fn list_message_tags(
        &self,
        request: Request<ListMessageTagsRequest>,
    ) -> Result<Response<ListMessageTagsResponse>, Status> {
        self.handler.handle_list_message_tags(request).await
    }

    async fn export_messages(
        &self,
        request: Request<ExportMessagesRequest>,
    ) -> Result<Response<ExportMessagesResponse>, Status> {
        self.handler.handle_export_messages(request).await
    }
}
