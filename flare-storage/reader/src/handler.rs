use std::sync::Arc;

use flare_proto::storage::*;
use tonic::{Request, Response, Status};
use tracing::error;

use crate::application::queries::query_messages::QueryMessagesService;
use crate::infrastructure::config::StorageReaderConfig;

pub struct StorageReaderHandler {
    query_service: Arc<QueryMessagesService>,
}

impl StorageReaderHandler {
    pub async fn new() -> anyhow::Result<Self> {
        let config = Arc::new(StorageReaderConfig::from_env());
        let query_service = Arc::new(QueryMessagesService::new(config).await?);

        Ok(Self { query_service })
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

    pub async fn handle_delete_message(
        &self,
        _request: Request<DeleteMessageRequest>,
    ) -> Result<Response<DeleteMessageResponse>, Status> {
        Err(Status::unimplemented(
            "delete_message is not yet implemented",
        ))
    }

    pub async fn handle_recall_message(
        &self,
        _request: Request<RecallMessageRequest>,
    ) -> Result<Response<RecallMessageResponse>, Status> {
        Err(Status::unimplemented(
            "recall_message is not yet implemented",
        ))
    }

    pub async fn handle_delete_message_for_user(
        &self,
        _request: Request<DeleteMessageForUserRequest>,
    ) -> Result<Response<DeleteMessageForUserResponse>, Status> {
        Err(Status::unimplemented(
            "delete_message_for_user is not yet implemented",
        ))
    }

    pub async fn handle_clear_session(
        &self,
        _request: Request<ClearSessionRequest>,
    ) -> Result<Response<ClearSessionResponse>, Status> {
        Err(Status::unimplemented(
            "clear_session is not yet implemented",
        ))
    }

    pub async fn handle_mark_message_read(
        &self,
        _request: Request<MarkMessageReadRequest>,
    ) -> Result<Response<MarkMessageReadResponse>, Status> {
        Err(Status::unimplemented(
            "mark_message_read is not yet implemented",
        ))
    }

    pub async fn handle_create_session(
        &self,
        _request: Request<CreateSessionRequest>,
    ) -> Result<Response<CreateSessionResponse>, Status> {
        Err(Status::unimplemented(
            "create_session is not yet implemented",
        ))
    }

    pub async fn handle_get_session(
        &self,
        _request: Request<GetSessionRequest>,
    ) -> Result<Response<GetSessionResponse>, Status> {
        Err(Status::unimplemented("get_session is not yet implemented"))
    }

    pub async fn handle_update_session(
        &self,
        _request: Request<UpdateSessionRequest>,
    ) -> Result<Response<UpdateSessionResponse>, Status> {
        Err(Status::unimplemented(
            "update_session is not yet implemented",
        ))
    }

    pub async fn handle_query_user_sessions(
        &self,
        _request: Request<QueryUserSessionsRequest>,
    ) -> Result<Response<QueryUserSessionsResponse>, Status> {
        Err(Status::unimplemented(
            "query_user_sessions is not yet implemented",
        ))
    }
}
