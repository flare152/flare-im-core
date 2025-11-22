use std::sync::Arc;

use flare_im_core::error::ok_status;
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
use flare_proto::storage::storage_reader_service_client::StorageReaderServiceClient;
use flare_proto::storage::{
    DeleteMessageRequest, GetMessageRequest, MarkMessageReadRequest, QueryMessagesRequest,
    RecallMessageRequest, SearchMessagesRequest, StoreMessageRequest,
};
use prost_types;
use tonic::{Request, Response, Status};
use tracing::{error, info, instrument};

use crate::application::commands::StoreMessageCommand;
use crate::application::handlers::MessageCommandHandler;
use flare_proto::message::message_service_server::MessageService;

/// 消息 gRPC 处理器 - 处理所有消息相关的 gRPC 请求（接口层）
/// 
/// 职责：
/// 1. 将 gRPC 请求转换为领域命令/查询
/// 2. 调用应用层 handlers
/// 3. 构建 gRPC 响应
#[derive(Clone)]
pub struct MessageGrpcHandler {
    command_handler: Arc<MessageCommandHandler>,
    reader_client: Option<StorageReaderServiceClient<tonic::transport::Channel>>,
}

impl MessageGrpcHandler {
    pub fn new(
        command_handler: Arc<MessageCommandHandler>,
        reader_client: Option<StorageReaderServiceClient<tonic::transport::Channel>>,
    ) -> Self {
        Self {
            command_handler,
            reader_client,
        }
    }

    /// 处理 SendMessage 请求
    #[instrument(skip(self, request))]
    pub async fn send_message(
        &self,
        request: Request<SendMessageRequest>,
    ) -> Result<Response<SendMessageResponse>, Status> {
        let req = request.into_inner();

        // 将 SendMessageRequest 转换为 StoreMessageRequest
        let store_request = StoreMessageRequest {
            session_id: req.session_id.clone(),
            message: req.message,
            sync: req.sync,
            context: req.context,
            tenant: req.tenant,
            tags: std::collections::HashMap::new(),
        };

        match self
            .command_handler
            .handle_store_message(StoreMessageCommand {
                request: store_request,
            })
            .await
        {
            Ok(message_id) => {
                // 构建时间线信息
                let now = chrono::Utc::now();
                let timeline = Some(flare_proto::common::MessageTimeline {
                    ingestion_time: Some(prost_types::Timestamp {
                        seconds: now.timestamp(),
                        nanos: now.timestamp_subsec_nanos() as i32,
                    }),
                    persisted_time: None,
                    delivered_time: None,
                    read_time: None,
                });

                Ok(Response::new(SendMessageResponse {
                    success: true,
                    message_id,
                    sent_at: Some(prost_types::Timestamp {
                        seconds: now.timestamp(),
                        nanos: now.timestamp_subsec_nanos() as i32,
                    }),
                    timeline,
                    status: Some(ok_status()),
                }))
            }
            Err(err) => {
                error!(error = %err, "Failed to orchestrate message");
                Err(Status::internal(err.to_string()))
            }
        }
    }

    /// 处理 BatchSendMessage 请求
    #[instrument(skip(self, request))]
    pub async fn batch_send_message(
        &self,
        request: Request<BatchSendMessageRequest>,
    ) -> Result<Response<BatchSendMessageResponse>, Status> {
        let req = request.into_inner();
        
        info!(
            "Orchestrator received batch: {} messages",
            req.messages.len()
        );
        
        let mut success_count = 0;
        let mut fail_count = 0;
        let mut message_ids = Vec::new();
        let mut failures = Vec::new();

        for send_req in req.messages {
            let store_request = StoreMessageRequest {
                session_id: send_req.session_id.clone(),
                message: send_req.message,
                sync: send_req.sync,
                context: send_req.context,
                tenant: send_req.tenant,
                tags: std::collections::HashMap::new(),
            };

            match self
                .command_handler
                .handle_store_message(StoreMessageCommand { request: store_request })
                .await
            {
                Ok(message_id) => {
                    success_count += 1;
                    message_ids.push(message_id);
                }
                Err(err) => {
                    fail_count += 1;
                    failures.push(flare_proto::message::FailedMessage {
                        message_id: String::new(),
                        code: 500, // InternalError
                        error_message: err.to_string(),
                    });
                }
            }
        }

        Ok(Response::new(BatchSendMessageResponse {
            success_count,
            fail_count,
            message_ids,
            failures,
            status: Some(ok_status()),
        }))
    }

    /// 处理 SendSystemMessage 请求
    /// 系统消息跳过 PreSend Hook 校验，但保留 PostSend Hook（用于通知业务系统）
    #[instrument(skip(self, request))]
    pub async fn send_system_message(
        &self,
        request: Request<SendSystemMessageRequest>,
    ) -> Result<Response<SendSystemMessageResponse>, Status> {
        let req = request.into_inner();

        // 验证必需字段
        if req.session_id.is_empty() {
            return Err(Status::invalid_argument("session_id is required"));
        }

        if req.message.is_none() {
            return Err(Status::invalid_argument("message is required"));
        }

        if req.system_message_type.is_empty() {
            return Err(Status::invalid_argument("system_message_type is required"));
        }

        // 构建 StoreMessageRequest，添加系统消息类型标签
        let mut tags = std::collections::HashMap::new();
        tags.insert("system_message_type".to_string(), req.system_message_type.clone());
        tags.insert("is_system_message".to_string(), "true".to_string());

        let mut message = req.message.unwrap();
        // 确保消息类型标记为系统消息
        message
            .extra
            .insert("system_message_type".to_string(), req.system_message_type.clone());
        message
            .extra
            .insert("sender_type".to_string(), "system".to_string());

        let store_request = StoreMessageRequest {
            session_id: req.session_id.clone(),
            message: Some(message),
            sync: false, // 系统消息默认异步
            context: req.context,
            tenant: req.tenant,
            tags,
        };

        // 调用 command_handler，跳过 PreSend Hook
        match self
            .command_handler
            .handle_store_message_without_pre_hook(StoreMessageCommand {
                request: store_request,
            })
            .await
        {
            Ok(message_id) => {
                info!(
                    message_id = %message_id,
                    session_id = %req.session_id,
                    system_message_type = %req.system_message_type,
                    "System message sent successfully"
                );
                Ok(Response::new(SendSystemMessageResponse {
                    success: true,
                    message_id,
                    status: Some(ok_status()),
                }))
            }
            Err(err) => {
                error!(
                    error = %err,
                    session_id = %req.session_id,
                    system_message_type = %req.system_message_type,
                    "Failed to send system message"
                );
                Err(Status::internal(err.to_string()))
            }
        }
    }

    /// 处理 QueryMessages 请求 - 转发到 StorageReaderService
    #[instrument(skip(self, request))]
    pub async fn query_messages(
        &self,
        request: Request<MessageQueryMessagesRequest>,
    ) -> Result<Response<MessageQueryMessagesResponse>, Status> {
        let req = request.into_inner();

        let client = self.reader_client.as_ref().ok_or_else(|| {
            Status::failed_precondition("Storage Reader not configured")
        })?;

        let storage_req = QueryMessagesRequest {
            session_id: req.session_id,
            start_time: req.start_time,
            end_time: req.end_time,
            limit: req.limit,
            cursor: req.cursor,
            context: req.context,
            tenant: req.tenant,
            pagination: req.pagination,
        };

        let storage_resp = client
            .clone()
            .query_messages(Request::new(storage_req))
            .await
            .map_err(|err| {
                error!(error = ?err, "Failed to query messages from reader");
                Status::internal("query storage reader failed")
            })?
            .into_inner();

        Ok(Response::new(MessageQueryMessagesResponse {
            messages: storage_resp.messages,
            next_cursor: storage_resp.next_cursor,
            has_more: storage_resp.has_more,
            pagination: storage_resp.pagination,
            status: storage_resp.status,
        }))
    }

    /// 处理 SearchMessages 请求 - 转发到 StorageReaderService
    #[instrument(skip(self, request))]
    pub async fn search_messages(
        &self,
        request: Request<MessageSearchMessagesRequest>,
    ) -> Result<Response<MessageSearchMessagesResponse>, Status> {
        let req = request.into_inner();

        let client = self.reader_client.as_ref().ok_or_else(|| {
            Status::failed_precondition("Storage Reader not configured")
        })?;

        let storage_req = SearchMessagesRequest {
            context: req.context,
            tenant: req.tenant,
            filters: req.filters,
            sort: req.sort,
            pagination: req.pagination,
            time_range: req.time_range,
        };

        let storage_resp = client
            .clone()
            .search_messages(Request::new(storage_req))
            .await
            .map_err(|err| {
                error!(error = ?err, "Failed to search messages from reader");
                Status::internal("search storage reader failed")
            })?
            .into_inner();

        Ok(Response::new(MessageSearchMessagesResponse {
            messages: storage_resp.messages,
            pagination: storage_resp.pagination,
            status: storage_resp.status,
        }))
    }

    /// 处理 GetMessage 请求 - 转发到 StorageReaderService
    #[instrument(skip(self, request))]
    pub async fn get_message(
        &self,
        request: Request<MessageGetMessageRequest>,
    ) -> Result<Response<MessageGetMessageResponse>, Status> {
        let req = request.into_inner();

        let client = self.reader_client.as_ref().ok_or_else(|| {
            Status::failed_precondition("Storage Reader not configured")
        })?;

        let storage_req = GetMessageRequest {
            context: req.context,
            tenant: req.tenant,
            message_id: req.message_id,
        };

        let storage_resp = client
            .clone()
            .get_message(Request::new(storage_req))
            .await
            .map_err(|err| {
                error!(error = ?err, "Failed to get message from reader");
                Status::internal("get message from reader failed")
            })?
            .into_inner();

        Ok(Response::new(MessageGetMessageResponse {
            message: storage_resp.message,
            status: storage_resp.status,
        }))
    }

    /// 处理 RecallMessage 请求 - 转发到 StorageReaderService
    #[instrument(skip(self, request))]
    pub async fn recall_message(
        &self,
        request: Request<MessageRecallMessageRequest>,
    ) -> Result<Response<MessageRecallMessageResponse>, Status> {
        let req = request.into_inner();

        let client = self.reader_client.as_ref().ok_or_else(|| {
            Status::failed_precondition("Storage Reader not configured")
        })?;

        let storage_req = RecallMessageRequest {
            message_id: req.message_id,
            operator_id: req.operator_id,
            reason: req.reason,
            recall_time_limit_seconds: req.recall_time_limit_seconds,
            context: req.context,
            tenant: req.tenant,
        };

        let storage_resp = client
            .clone()
            .recall_message(Request::new(storage_req))
            .await
            .map_err(|err| {
                error!(error = ?err, "Failed to recall message from reader");
                Status::internal("recall message from reader failed")
            })?
            .into_inner();

        Ok(Response::new(MessageRecallMessageResponse {
            success: storage_resp.success,
            error_message: storage_resp.error_message,
            recalled_at: storage_resp.recalled_at,
            status: storage_resp.status,
        }))
    }

    /// 处理 DeleteMessage 请求 - 转发到 StorageReaderService
    #[instrument(skip(self, request))]
    pub async fn delete_message(
        &self,
        request: Request<MessageDeleteMessageRequest>,
    ) -> Result<Response<MessageDeleteMessageResponse>, Status> {
        let req = request.into_inner();

        let client = self.reader_client.as_ref().ok_or_else(|| {
            Status::failed_precondition("Storage Reader not configured")
        })?;

        let storage_req = DeleteMessageRequest {
            session_id: req.session_id,
            message_ids: req.message_ids,
            context: req.context,
            tenant: req.tenant,
        };

        let storage_resp = client
            .clone()
            .delete_message(Request::new(storage_req))
            .await
            .map_err(|err| {
                error!(error = ?err, "Failed to delete message from reader");
                Status::internal("delete message from reader failed")
            })?
            .into_inner();

        Ok(Response::new(MessageDeleteMessageResponse {
            success: storage_resp.success,
            deleted_count: storage_resp.deleted_count,
            status: storage_resp.status,
        }))
    }

    /// 处理 MarkMessageRead 请求 - 转发到 StorageReaderService
    #[instrument(skip(self, request))]
    pub async fn mark_message_read(
        &self,
        request: Request<MessageMarkMessageReadRequest>,
    ) -> Result<Response<MessageMarkMessageReadResponse>, Status> {
        let req = request.into_inner();

        let client = self.reader_client.as_ref().ok_or_else(|| {
            Status::failed_precondition("Storage Reader not configured")
        })?;

        let storage_req = MarkMessageReadRequest {
            message_id: req.message_id,
            user_id: req.user_id,
            context: req.context,
            tenant: req.tenant,
        };

        let storage_resp = client
            .clone()
            .mark_message_read(Request::new(storage_req))
            .await
            .map_err(|err| {
                error!(error = ?err, "Failed to mark message read from reader");
                Status::internal("mark message read from reader failed")
            })?
            .into_inner();

        Ok(Response::new(MessageMarkMessageReadResponse {
            success: storage_resp.success,
            error_message: storage_resp.error_message,
            read_at: storage_resp.read_at,
            burned_at: storage_resp.burned_at,
            status: storage_resp.status,
        }))
    }
}

#[tonic::async_trait]
impl MessageService for MessageGrpcHandler {
    async fn send_message(
        &self,
        request: Request<SendMessageRequest>,
    ) -> Result<Response<SendMessageResponse>, Status> {
        self.send_message(request).await
    }

    async fn batch_send_message(
        &self,
        request: Request<BatchSendMessageRequest>,
    ) -> Result<Response<BatchSendMessageResponse>, Status> {
        self.batch_send_message(request).await
    }

    async fn send_system_message(
        &self,
        request: Request<SendSystemMessageRequest>,
    ) -> Result<Response<SendSystemMessageResponse>, Status> {
        self.send_system_message(request).await
    }

    async fn query_messages(
        &self,
        request: Request<MessageQueryMessagesRequest>,
    ) -> Result<Response<MessageQueryMessagesResponse>, Status> {
        self.query_messages(request).await
    }

    async fn search_messages(
        &self,
        request: Request<MessageSearchMessagesRequest>,
    ) -> Result<Response<MessageSearchMessagesResponse>, Status> {
        self.search_messages(request).await
    }

    async fn get_message(
        &self,
        request: Request<MessageGetMessageRequest>,
    ) -> Result<Response<MessageGetMessageResponse>, Status> {
        self.get_message(request).await
    }

    async fn recall_message(
        &self,
        request: Request<MessageRecallMessageRequest>,
    ) -> Result<Response<MessageRecallMessageResponse>, Status> {
        self.recall_message(request).await
    }

    async fn delete_message(
        &self,
        request: Request<MessageDeleteMessageRequest>,
    ) -> Result<Response<MessageDeleteMessageResponse>, Status> {
        self.delete_message(request).await
    }

    async fn mark_message_read(
        &self,
        request: Request<MessageMarkMessageReadRequest>,
    ) -> Result<Response<MessageMarkMessageReadResponse>, Status> {
        self.mark_message_read(request).await
    }
}
