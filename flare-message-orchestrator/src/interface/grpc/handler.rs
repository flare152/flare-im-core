use std::sync::Arc;

use flare_proto::message::message_service_server::MessageService;
use flare_proto::message::{
    BatchSendMessageRequest, BatchSendMessageResponse,
    DeleteMessageRequest as MessageDeleteMessageRequest,
    DeleteMessageResponse as MessageDeleteMessageResponse,
    GetMessageRequest as MessageGetMessageRequest,
    GetMessageResponse as MessageGetMessageResponse,
    MarkMessageReadRequest as MessageMarkMessageReadRequest,
    MarkMessageReadResponse as MessageMarkMessageReadResponse,
    QueryMessagesRequest as MessageQueryMessagesRequest,
    QueryMessagesResponse as MessageQueryMessagesResponse,
    RecallMessageRequest as MessageRecallMessageRequest,
    RecallMessageResponse as MessageRecallMessageResponse,
    SearchMessagesRequest as MessageSearchMessagesRequest,
    SearchMessagesResponse as MessageSearchMessagesResponse,
    SendMessageRequest,
    SendMessageResponse,
    SendSystemMessageRequest,
    SendSystemMessageResponse,
};
use tonic::{Request, Response, Status};
use tracing::{info, instrument};

use crate::application::commands::MessageCommandService;

#[derive(Clone)]
pub struct MessageGrpcHandler {
    command_service: Arc<MessageCommandService>,
}

impl MessageGrpcHandler {
    pub fn new(command_service: Arc<MessageCommandService>) -> Self {
        Self { command_service }
    }
}

#[tonic::async_trait]
impl MessageService for MessageGrpcHandler {
    #[instrument(skip(self, request))]
    async fn send_message(
        &self,
        request: Request<SendMessageRequest>,
    ) -> Result<Response<SendMessageResponse>, Status> {
        info!(
            "Orchestrator received message: session_id={}",
            request.get_ref().session_id
        );
        self.command_service.send_message(request).await
    }

    #[instrument(skip(self, request))]
    async fn batch_send_message(
        &self,
        request: Request<BatchSendMessageRequest>,
    ) -> Result<Response<BatchSendMessageResponse>, Status> {
        info!(
            "Orchestrator received batch: {} messages",
            request.get_ref().messages.len()
        );
        self.command_service.batch_send_message(request).await
    }

    #[instrument(skip(self, request))]
    async fn send_system_message(
        &self,
        request: Request<SendSystemMessageRequest>,
    ) -> Result<Response<SendSystemMessageResponse>, Status> {
        self.command_service.send_system_message(request).await
    }

    #[instrument(skip(self, request))]
    async fn query_messages(
        &self,
        request: Request<MessageQueryMessagesRequest>,
    ) -> Result<Response<MessageQueryMessagesResponse>, Status> {
        self.command_service.query_messages(request).await
    }

    #[instrument(skip(self, request))]
    async fn search_messages(
        &self,
        request: Request<MessageSearchMessagesRequest>,
    ) -> Result<Response<MessageSearchMessagesResponse>, Status> {
        self.command_service.search_messages(request).await
    }

    #[instrument(skip(self, request))]
    async fn get_message(
        &self,
        request: Request<MessageGetMessageRequest>,
    ) -> Result<Response<MessageGetMessageResponse>, Status> {
        self.command_service.get_message(request).await
    }

    #[instrument(skip(self, request))]
    async fn recall_message(
        &self,
        request: Request<MessageRecallMessageRequest>,
    ) -> Result<Response<MessageRecallMessageResponse>, Status> {
        self.command_service.recall_message(request).await
    }

    #[instrument(skip(self, request))]
    async fn delete_message(
        &self,
        request: Request<MessageDeleteMessageRequest>,
    ) -> Result<Response<MessageDeleteMessageResponse>, Status> {
        self.command_service.delete_message(request).await
    }

    #[instrument(skip(self, request))]
    async fn mark_message_read(
        &self,
        request: Request<MessageMarkMessageReadRequest>,
    ) -> Result<Response<MessageMarkMessageReadResponse>, Status> {
        self.command_service.mark_message_read(request).await
    }
}

