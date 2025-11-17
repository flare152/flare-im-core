use std::sync::Arc;

use flare_proto::storage::*;
use tonic::{Request, Response, Status};
use tracing::error;

use crate::application::commands::{
    ClearSessionService, DeleteMessageService, DeleteMessageForUserService, MarkReadService,
    RecallMessageService, SetMessageAttributesService, ExportMessagesService,
};
use crate::application::queries::{
    GetMessageService, QueryMessagesService, SearchMessagesService, ListMessageTagsService,
};
use crate::config::StorageReaderConfig;
use crate::infrastructure::persistence::mongo::MongoMessageStorage;

pub struct StorageReaderHandler {
    query_service: Arc<QueryMessagesService>,
    get_message_service: Arc<GetMessageService>,
    search_messages_service: Arc<SearchMessagesService>,
    delete_message_service: Arc<DeleteMessageService>,
    recall_message_service: Arc<RecallMessageService>,
    delete_message_for_user_service: Arc<DeleteMessageForUserService>,
    clear_session_service: Arc<ClearSessionService>,
    mark_read_service: Arc<MarkReadService>,
    set_message_attributes_service: Arc<SetMessageAttributesService>,
    list_message_tags_service: Arc<ListMessageTagsService>,
    export_messages_service: Arc<ExportMessagesService>,
}

impl StorageReaderHandler {
    pub async fn new(app_config: &flare_im_core::config::FlareAppConfig) -> anyhow::Result<Self> {
        use anyhow::Context;
        let config = Arc::new(
            StorageReaderConfig::from_app_config(app_config)
                .context("Failed to load storage reader configuration")?,
        );
        
        // 创建存储实例
        let storage = if let Some(url) = &config.mongo_url {
            Arc::new(
                MongoMessageStorage::new(url, &config.mongo_database)
                    .await
                    .unwrap_or_default(),
            )
        } else {
            Arc::new(MongoMessageStorage::default())
        };

        // 创建查询服务
        let query_service = Arc::new(QueryMessagesService::new(config.clone()).await?);
        let search_messages_service = Arc::new(SearchMessagesService::new(config.clone()).await?);
        let get_message_service = Arc::new(GetMessageService::new(storage.clone()));
        let list_message_tags_service = Arc::new(ListMessageTagsService::new(storage.clone()));

        // 创建命令服务
        let delete_message_service = Arc::new(DeleteMessageService::new(storage.clone()));
        let recall_message_service = Arc::new(RecallMessageService::new(storage.clone()));
        let delete_message_for_user_service = Arc::new(DeleteMessageForUserService::new(storage.clone()));
        let clear_session_service = Arc::new(ClearSessionService::new(storage.clone()));
        let mark_read_service = Arc::new(MarkReadService::new(storage.clone()));
        let set_message_attributes_service = Arc::new(SetMessageAttributesService::new(storage.clone()));
        let export_messages_service = Arc::new(ExportMessagesService::new(config.clone()).await?);

        Ok(Self {
            query_service,
            get_message_service,
            search_messages_service,
            delete_message_service,
            recall_message_service,
            delete_message_for_user_service,
            clear_session_service,
            mark_read_service,
            set_message_attributes_service,
            list_message_tags_service,
            export_messages_service,
        })
    }

    pub async fn handle_query_messages(
        &self,
        request: Request<QueryMessagesRequest>,
    ) -> Result<Response<QueryMessagesResponse>, Status> {
        match self.query_service.execute(request.into_inner()).await {
            Ok(response) => Ok(Response::new(response)),
            Err(err) => {
                error!(error = ?err, "Failed to query messages");
                Err(Status::internal(err.to_string()))
            }
        }
    }

    pub async fn handle_get_message(
        &self,
        request: Request<GetMessageRequest>,
    ) -> Result<Response<GetMessageResponse>, Status> {
        match self.get_message_service.execute(request.into_inner()).await {
            Ok(response) => Ok(Response::new(response)),
            Err(err) => {
                error!(error = ?err, "Failed to get message");
                Err(Status::internal(err.to_string()))
            }
        }
    }

    pub async fn handle_delete_message(
        &self,
        request: Request<DeleteMessageRequest>,
    ) -> Result<Response<DeleteMessageResponse>, Status> {
        match self.delete_message_service.execute(request.into_inner()).await {
            Ok(response) => Ok(Response::new(response)),
            Err(err) => {
                error!(error = ?err, "Failed to delete message");
                Err(Status::internal(err.to_string()))
            }
        }
    }

    pub async fn handle_recall_message(
        &self,
        request: Request<RecallMessageRequest>,
    ) -> Result<Response<RecallMessageResponse>, Status> {
        match self.recall_message_service.execute(request.into_inner()).await {
            Ok(response) => Ok(Response::new(response)),
            Err(err) => {
                error!(error = ?err, "Failed to recall message");
                Err(Status::internal(err.to_string()))
            }
        }
    }

    pub async fn handle_delete_message_for_user(
        &self,
        request: Request<DeleteMessageForUserRequest>,
    ) -> Result<Response<DeleteMessageForUserResponse>, Status> {
        match self
            .delete_message_for_user_service
            .execute(request.into_inner())
            .await
        {
            Ok(response) => Ok(Response::new(response)),
            Err(err) => {
                error!(error = ?err, "Failed to delete message for user");
                Err(Status::internal(err.to_string()))
            }
        }
    }

    pub async fn handle_clear_session(
        &self,
        request: Request<ClearSessionRequest>,
    ) -> Result<Response<ClearSessionResponse>, Status> {
        match self.clear_session_service.execute(request.into_inner()).await {
            Ok(response) => Ok(Response::new(response)),
            Err(err) => {
                error!(error = ?err, "Failed to clear session");
                Err(Status::internal(err.to_string()))
            }
        }
    }

    pub async fn handle_mark_message_read(
        &self,
        request: Request<MarkMessageReadRequest>,
    ) -> Result<Response<MarkMessageReadResponse>, Status> {
        match self.mark_read_service.execute(request.into_inner()).await {
            Ok(response) => Ok(Response::new(response)),
            Err(err) => {
                error!(error = ?err, "Failed to mark message as read");
                Err(Status::internal(err.to_string()))
            }
        }
    }

    pub async fn handle_search_messages(
        &self,
        request: Request<SearchMessagesRequest>,
    ) -> Result<Response<SearchMessagesResponse>, Status> {
        match self
            .search_messages_service
            .execute(request.into_inner())
            .await
        {
            Ok(response) => Ok(Response::new(response)),
            Err(err) => {
                error!(error = ?err, "Failed to search messages");
                Err(Status::internal(err.to_string()))
            }
        }
    }

    pub async fn handle_set_message_attributes(
        &self,
        request: Request<SetMessageAttributesRequest>,
    ) -> Result<Response<SetMessageAttributesResponse>, Status> {
        match self
            .set_message_attributes_service
            .execute(request.into_inner())
            .await
        {
            Ok(response) => Ok(Response::new(response)),
            Err(err) => {
                error!(error = ?err, "Failed to set message attributes");
                Err(Status::internal(err.to_string()))
            }
        }
    }

    pub async fn handle_list_message_tags(
        &self,
        request: Request<ListMessageTagsRequest>,
    ) -> Result<Response<ListMessageTagsResponse>, Status> {
        match self
            .list_message_tags_service
            .execute(request.into_inner())
            .await
        {
            Ok(response) => Ok(Response::new(response)),
            Err(err) => {
                error!(error = ?err, "Failed to list message tags");
                Err(Status::internal(err.to_string()))
            }
        }
    }

    pub async fn handle_export_messages(
        &self,
        request: Request<ExportMessagesRequest>,
    ) -> Result<Response<ExportMessagesResponse>, Status> {
        match self
            .export_messages_service
            .execute(request.into_inner())
            .await
        {
            Ok(response) => Ok(Response::new(response)),
            Err(err) => {
                error!(error = ?err, "Failed to export messages");
                Err(Status::internal(err.to_string()))
            }
        }
    }

    // 注意：会话管理方法不属于 StorageService，已移除
}
