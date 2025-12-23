use std::sync::Arc;

use flare_im_core::error::ok_status;
use flare_proto::message::{
    AddReactionRequest as MessageAddReactionRequest,
    AddReactionResponse as MessageAddReactionResponse,
    AddThreadReplyRequest as MessageAddThreadReplyRequest,
    AddThreadReplyResponse as MessageAddThreadReplyResponse,
    BatchMarkMessageReadRequest as MessageBatchMarkMessageReadRequest,
    BatchMarkMessageReadResponse as MessageBatchMarkMessageReadResponse, BatchSendMessageRequest,
    BatchSendMessageResponse, DeleteMessageRequest as MessageDeleteMessageRequest,
    DeleteMessageResponse as MessageDeleteMessageResponse,
    EditMessageRequest as MessageEditMessageRequest,
    EditMessageResponse as MessageEditMessageResponse,
    FavoriteMessageRequest as MessageFavoriteMessageRequest,
    FavoriteMessageResponse as MessageFavoriteMessageResponse,
    ForwardMessageRequest as MessageForwardMessageRequest,
    ForwardMessageResponse as MessageForwardMessageResponse,
    GetMessageRequest as MessageGetMessageRequest, GetMessageResponse as MessageGetMessageResponse,
    MarkMessageReadRequest as MessageMarkMessageReadRequest,
    MarkMessageReadResponse as MessageMarkMessageReadResponse,
    MarkMessageRequest as MessageMarkMessageRequest,
    MarkMessageResponse as MessageMarkMessageResponse,
    PinMessageRequest as MessagePinMessageRequest, PinMessageResponse as MessagePinMessageResponse,
    QueryMessagesRequest as MessageQueryMessagesRequest,
    QueryMessagesResponse as MessageQueryMessagesResponse,
    QuoteMessageRequest as MessageQuoteMessageRequest,
    QuoteMessageResponse as MessageQuoteMessageResponse,
    RecallMessageRequest as MessageRecallMessageRequest,
    RecallMessageResponse as MessageRecallMessageResponse,
    RemoveReactionRequest as MessageRemoveReactionRequest,
    RemoveReactionResponse as MessageRemoveReactionResponse,
    ReplyMessageRequest as MessageReplyMessageRequest,
    ReplyMessageResponse as MessageReplyMessageResponse,
    SearchMessagesRequest as MessageSearchMessagesRequest,
    SearchMessagesResponse as MessageSearchMessagesResponse, SendMessageRequest,
    SendMessageResponse, SendSystemMessageRequest, SendSystemMessageResponse,
    UnfavoriteMessageRequest as MessageUnfavoriteMessageRequest,
    UnfavoriteMessageResponse as MessageUnfavoriteMessageResponse,
    UnpinMessageRequest as MessageUnpinMessageRequest,
    UnpinMessageResponse as MessageUnpinMessageResponse,
};
use flare_proto::storage::storage_reader_service_client::StorageReaderServiceClient;
use flare_proto::storage::{
    DeleteMessageRequest, GetMessageRequest, MarkMessageReadRequest, RecallMessageRequest,
    SetMessageAttributesRequest, StoreMessageRequest,
};
use prost::Message as ProstMessage;
use prost_types;
use tonic::{Request, Response, Status};
use tracing::{error, info, instrument, warn};

use crate::application::commands::StoreMessageCommand;
use crate::application::handlers::{MessageCommandHandler, MessageQueryHandler};
use crate::domain::repository::MessageEventPublisher;
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
    query_handler: Arc<MessageQueryHandler>,
    reader_client: Option<StorageReaderServiceClient<tonic::transport::Channel>>,
    publisher: Arc<crate::domain::repository::MessageEventPublisherItem>,
}

impl MessageGrpcHandler {
    pub fn new(
        command_handler: Arc<MessageCommandHandler>,
        query_handler: Arc<MessageQueryHandler>,
        reader_client: Option<StorageReaderServiceClient<tonic::transport::Channel>>,
        publisher: Arc<crate::domain::repository::MessageEventPublisherItem>,
    ) -> Self {
        Self {
            command_handler,
            query_handler,
            reader_client,
            publisher,
        }
    }

    /// 处理 SendMessage 请求（内部实现）
    #[instrument(skip(self, request))]
    async fn send_message_impl(
        &self,
        request: Request<SendMessageRequest>,
    ) -> Result<Response<SendMessageResponse>, Status> {
        let req = request.into_inner();
        let message = req
            .message
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("message required"))?;

        // 检查是否是操作消息（根据架构优化方案，操作消息通过 NotificationContent 传递）
        if message.message_type == flare_proto::MessageType::Notification as i32 {
            tracing::debug!(
                message_id = %message.server_id,
                message_type = message.message_type,
                "Received notification message, attempting to extract operation"
            );
            if let Some(operation) = self.extract_operation_from_message(message)? {
                tracing::info!(
                    operation_type = operation.operation_type,
                    target_message_id = %operation.target_message_id,
                    operator_id = %operation.operator_id,
                    "Extracted message operation, handling"
                );
                // 统一处理所有操作
                return self
                    .handle_message_operation(operation, message, &req)
                    .await;
            } else {
                tracing::warn!(
                    message_id = %message.server_id,
                    "Notification message but failed to extract operation"
                );
            }
        }

        // 普通消息处理（存储编排）
        self.handle_normal_message(message, &req).await
    }

    /// 内部方法：直接发送普通消息（不检查操作消息，避免递归）
    async fn send_normal_message_internal(
        &self,
        message: &flare_proto::Message,
        request: &SendMessageRequest,
    ) -> Result<Response<SendMessageResponse>, Status> {
        self.handle_normal_message(message, request).await
    }

    /// 从 Message 中提取 MessageOperation
    fn extract_operation_from_message(
        &self,
        message: &flare_proto::Message,
    ) -> Result<Option<flare_proto::common::MessageOperation>, Status> {
        use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
        use flare_proto::common::message_content::Content;

        // 从 NotificationContent 中提取 MessageOperation
        if let Some(ref content) = message.content {
            if let Some(Content::Notification(notif)) = &content.content {
                if notif.notification_type == "message_operation" {
                    // 从 data 中提取 operation_data（base64 编码）
                    if let Some(operation_data_str) = notif.data.get("operation_data") {
                        // 解码 base64
                        let operation_bytes = BASE64.decode(operation_data_str).map_err(|e| {
                            Status::invalid_argument(format!(
                                "Failed to decode operation_data: {}",
                                e
                            ))
                        })?;

                        // 解析 MessageOperation
                        let operation =
                            flare_proto::common::MessageOperation::decode(&operation_bytes[..])
                                .map_err(|e| {
                                    Status::invalid_argument(format!(
                                        "Failed to decode MessageOperation: {}",
                                        e
                                    ))
                                })?;

                        return Ok(Some(operation));
                    }
                }
            }
        }
        Ok(None)
    }

    /// 处理普通消息（存储编排）
    async fn handle_normal_message(
        &self,
        message: &flare_proto::Message,
        req: &SendMessageRequest,
    ) -> Result<Response<SendMessageResponse>, Status> {
        // 验证单聊消息必须包含 receiver_id
        if message.conversation_type == flare_proto::common::ConversationType::Single as i32 {
            if message.receiver_id.is_empty() {
                error!(
                    message_id = %message.server_id,
                    conversation_id = %message.conversation_id,
                    sender_id = %message.sender_id,
                    "Single chat message missing receiver_id in gRPC handler"
                );
                return Err(Status::invalid_argument(format!(
                    "Single chat message must provide receiver_id. message_id={}, conversation_id={}, sender_id={}",
                    message.server_id, message.conversation_id, message.sender_id
                )));
            }
        }

        // 将 SendMessageRequest 转换为 StoreMessageRequest
        let store_request = StoreMessageRequest {
            conversation_id: req.conversation_id.clone(),
            message: Some(message.clone()),
            sync: req.sync,
            context: req.context.clone(),
            tenant: req.tenant.clone(),
            tags: std::collections::HashMap::new(),
        };

        match self
            .command_handler
            .handle_store_message(StoreMessageCommand {
                request: store_request,
            })
            .await
        {
            Ok((message_id, seq)) => {
                // 构建时间线信息
                let now = chrono::Utc::now();
                let timeline = Some(flare_proto::common::MessageTimeline {
                    created_at: Some(prost_types::Timestamp {
                        seconds: now.timestamp(),
                        nanos: now.timestamp_subsec_nanos() as i32,
                    }),
                    persisted_at: None,
                    delivered_at: None,
                    read_at: None,
                });

                Ok(Response::new(SendMessageResponse {
                    success: true,
                    server_msg_id: message_id,
                    seq: seq,
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

    /// 统一处理所有操作消息
    async fn handle_message_operation(
        &self,
        operation: flare_proto::common::MessageOperation,
        message: &flare_proto::Message,
        request: &SendMessageRequest,
    ) -> Result<Response<SendMessageResponse>, Status> {
        use crate::domain::service::operation_classifier::OperationClassifier;

        let operation_type = operation.operation_type;

        // 操作分类
        if OperationClassifier::can_delegate_directly(operation_type) {
            // 简单操作：直接委托给 Storage Reader/Writer
            return self.handle_simple_operation(operation, request).await;
        } else if OperationClassifier::requires_orchestration(operation_type) {
            // 复杂操作：需要编排
            return self
                .handle_complex_operation(operation, message, request)
                .await;
        } else {
            return Err(Status::invalid_argument(format!(
                "Unsupported operation type: {}",
                operation_type
            )));
        }
    }

    /// 处理简单操作（直接委托给 Storage Reader/Writer）
    async fn handle_simple_operation(
        &self,
        operation: flare_proto::common::MessageOperation,
        request: &SendMessageRequest,
    ) -> Result<Response<SendMessageResponse>, Status> {
        use flare_proto::common::OperationType;
        use flare_proto::common::message_operation::OperationData;

        let client = self
            .reader_client
            .as_ref()
            .ok_or_else(|| Status::failed_precondition("Storage Reader not configured"))?;

        match OperationType::try_from(operation.operation_type) {
            Ok(OperationType::Read) => {
                // 标记已读：直接委托给 Storage Reader
                // 支持批量已读（ReadOperationData 包含 message_ids 列表）
                let read_data = match &operation.operation_data {
                    Some(OperationData::Read(data)) => data,
                    _ => {
                        return Err(Status::invalid_argument(
                            "Read operation requires ReadOperationData",
                        ));
                    }
                };

                // 批量标记已读（逐个处理，因为 Storage Reader 只支持单个消息）
                let mut success_count = 0;
                let mut failed_count = 0;
                
                for message_id in &read_data.message_ids {
                    let read_req = MarkMessageReadRequest {
                        message_id: message_id.clone(),
                        user_id: operation.operator_id.clone(),
                        context: request.context.clone(),
                        tenant: request.tenant.clone(),
                    };

                    match client.clone().mark_message_read(Request::new(read_req)).await {
                        Ok(resp) => {
                            if resp.into_inner().success {
                                success_count += 1;
                            } else {
                                failed_count += 1;
                            }
                        }
                        Err(_) => {
                            failed_count += 1;
                        }
                    }
                }

                // 转换为 SendMessageResponse（使用第一条消息ID作为目标）
                Ok(Response::new(SendMessageResponse {
                    success: success_count > 0,
                    server_msg_id: if !read_data.message_ids.is_empty() {
                        read_data.message_ids[0].clone()
                    } else {
                        operation.target_message_id.clone()
                    },
                    seq: 0, // 批量已读操作不返回 seq
                    sent_at: operation.timestamp.clone(),
                    timeline: None,
                    status: Some(ok_status()),
                }))
            }
            Ok(OperationType::ReactionAdd) | Ok(OperationType::ReactionRemove) => {
                // 反应操作：使用新的 AddOrRemoveReaction 接口（支持多用户反应列表）
                let reaction_data = match operation.operation_data {
                    Some(OperationData::Reaction(data)) => data,
                    _ => {
                        return Err(Status::invalid_argument(
                            "Reaction operation requires ReactionOperationData",
                        ));
                    }
                };

                let is_add = operation.operation_type == OperationType::ReactionAdd as i32;

                let reaction_req = flare_proto::storage::AddOrRemoveReactionRequest {
                    message_id: operation.target_message_id.clone(),
                    emoji: reaction_data.emoji.clone(),
                    user_id: operation.operator_id.clone(),
                    is_add,
                    context: request.context.clone(),
                    tenant: request.tenant.clone(),
                };

                let resp = client
                    .clone()
                    .add_or_remove_reaction(Request::new(reaction_req))
                    .await?;
                let inner = resp.into_inner();

                Ok(Response::new(SendMessageResponse {
                    success: inner.status.as_ref().map(|s| s.code == 0).unwrap_or(false),
                    server_msg_id: operation.target_message_id,
                    seq: 0, // 反应操作不返回 seq
                    sent_at: operation.timestamp.clone(),
                    timeline: None,
                    status: inner.status,
                }))
            }
            Ok(OperationType::Pin) | Ok(OperationType::Unpin) => {
                // 置顶操作：通过 SetMessageAttributes
                let pin_key = "pinned".to_string();
                let pin_value = if operation.operation_type == OperationType::Pin as i32 {
                    "true".to_string()
                } else {
                    "false".to_string()
                };

                let mut attributes = std::collections::HashMap::new();
                attributes.insert(pin_key, pin_value);

                let set_req = SetMessageAttributesRequest {
                    context: request.context.clone(),
                    tenant: request.tenant.clone(),
                    message_id: operation.target_message_id.clone(),
                    attributes,
                    tags: vec![],
                };

                let resp = client
                    .clone()
                    .set_message_attributes(Request::new(set_req))
                    .await?;
                let inner = resp.into_inner();

                Ok(Response::new(SendMessageResponse {
                    success: inner.status.is_some() && inner.status.as_ref().unwrap().code == 0,
                    server_msg_id: operation.target_message_id,
                    seq: 0, // 置顶操作不返回 seq
                    sent_at: operation.timestamp.clone(),
                    timeline: None,
                    status: inner.status,
                }))
            }
            Ok(OperationType::Favorite) | Ok(OperationType::Unfavorite) => {
                // 收藏操作：通过 SetMessageAttributes
                let favorite_key = format!("favorited_by:{}", operation.operator_id);
                let favorite_value = if operation.operation_type == OperationType::Favorite as i32 {
                    "true".to_string()
                } else {
                    "false".to_string()
                };

                let mut attributes = std::collections::HashMap::new();
                attributes.insert(favorite_key, favorite_value);

                let set_req = SetMessageAttributesRequest {
                    context: request.context.clone(),
                    tenant: request.tenant.clone(),
                    message_id: operation.target_message_id.clone(),
                    attributes,
                    tags: vec![],
                };

                let resp = client
                    .clone()
                    .set_message_attributes(Request::new(set_req))
                    .await?;
                let inner = resp.into_inner();

                Ok(Response::new(SendMessageResponse {
                    success: inner.status.is_some() && inner.status.as_ref().unwrap().code == 0,
                    server_msg_id: operation.target_message_id,
                    seq: 0, // 置顶操作不返回 seq
                    sent_at: operation.timestamp.clone(),
                    timeline: None,
                    status: inner.status,
                }))
            }
            Ok(OperationType::Mark) => {
                // 标记操作：通过 SetMessageAttributes
                let mark_data = match operation.operation_data {
                    Some(OperationData::Mark(data)) => data,
                    _ => {
                        return Err(Status::invalid_argument(
                            "Mark operation requires MarkOperationData",
                        ));
                    }
                };

                let mark_key = format!(
                    "mark:{}:{}",
                    match mark_data.mark_type {
                        0 => "important", // MarkType::Important
                        1 => "todo",      // MarkType::Todo
                        2 => "processed", // MarkType::Done
                        3 => "custom",    // MarkType::Custom
                        _ => "unknown",
                    },
                    operation.operator_id
                );

                let mut attributes = std::collections::HashMap::new();
                attributes.insert(mark_key, "true".to_string());

                let set_req = SetMessageAttributesRequest {
                    context: request.context.clone(),
                    tenant: request.tenant.clone(),
                    message_id: operation.target_message_id.clone(),
                    attributes,
                    tags: vec![],
                };

                let resp = client
                    .clone()
                    .set_message_attributes(Request::new(set_req))
                    .await?;
                let inner = resp.into_inner();

                Ok(Response::new(SendMessageResponse {
                    success: inner.status.is_some() && inner.status.as_ref().unwrap().code == 0,
                    server_msg_id: operation.target_message_id,
                    seq: 0, // 置顶操作不返回 seq
                    sent_at: operation.timestamp.clone(),
                    timeline: None,
                    status: inner.status,
                }))
            }
            Ok(OperationType::Delete) => {
                // 删除操作：直接委托
                let _delete_data = match operation.operation_data {
                    Some(OperationData::Delete(data)) => data,
                    _ => {
                        return Err(Status::invalid_argument(
                            "Delete operation requires DeleteOperationData",
                        ));
                    }
                };

                let delete_req = DeleteMessageRequest {
                    conversation_id: request.conversation_id.clone(),
                    message_ids: vec![operation.target_message_id.clone()],
                    context: request.context.clone(),
                    tenant: request.tenant.clone(),
                };

                let resp = client
                    .clone()
                    .delete_message(Request::new(delete_req))
                    .await?;
                let inner = resp.into_inner();

                Ok(Response::new(SendMessageResponse {
                    success: inner.success,
                    server_msg_id: operation.target_message_id,
                    seq: 0, // 删除操作不返回 seq
                    sent_at: operation.timestamp.clone(),
                    timeline: None,
                    status: inner.status,
                }))
            }
            _ => Err(Status::invalid_argument("Not a simple operation")),
        }
    }

    /// 处理复杂操作（需要编排）
    async fn handle_complex_operation(
        &self,
        operation: flare_proto::common::MessageOperation,
        message: &flare_proto::Message,
        request: &SendMessageRequest,
    ) -> Result<Response<SendMessageResponse>, Status> {
        use flare_proto::common::OperationType;
        use flare_proto::common::message_operation::OperationData;

        match OperationType::try_from(operation.operation_type) {
            Ok(OperationType::Recall) => {
                // 撤回：需要编排（权限验证 → 状态更新 → 事件发布）
                let recall_data = match operation.operation_data {
                    Some(OperationData::Recall(data)) => data,
                    _ => {
                        return Err(Status::invalid_argument(
                            "Recall operation requires RecallOperationData",
                        ));
                    }
                };

                let recall_req = MessageRecallMessageRequest {
                    message_id: operation.target_message_id.clone(),
                    operator_id: operation.operator_id.clone(),
                    reason: recall_data.reason,
                    recall_time_limit_seconds: recall_data.time_limit_seconds,
                    context: request.context.clone(),
                    tenant: request.tenant.clone(),
                };

                let resp = self.recall_message(Request::new(recall_req)).await?;
                let inner = resp.into_inner();

                Ok(Response::new(SendMessageResponse {
                    success: inner.success,
                    server_msg_id: operation.target_message_id,
                    seq: 0, // 删除操作不返回 seq
                    sent_at: operation.timestamp.clone(),
                    timeline: None,
                    status: inner.status,
                }))
            }
            Ok(OperationType::Edit) => {
                // 编辑：需要编排（权限验证 → 内容更新 → 事件发布）
                let edit_data = match operation.operation_data {
                    Some(OperationData::Edit(data)) => data,
                    _ => {
                        return Err(Status::invalid_argument(
                            "Edit operation requires EditOperationData",
                        ));
                    }
                };

                let edit_req = MessageEditMessageRequest {
                    message_id: operation.target_message_id.clone(),
                    operator_id: operation.operator_id.clone(),
                    new_content: edit_data.new_content,
                    edit_version: edit_data.edit_version,
                    reason: edit_data.reason,
                    show_edited_mark: edit_data.show_edited_mark,
                    context: request.context.clone(),
                    tenant: request.tenant.clone(),
                };

                let resp = self.edit_message(Request::new(edit_req)).await?;
                let inner = resp.into_inner();

                Ok(Response::new(SendMessageResponse {
                    success: inner.success,
                    server_msg_id: operation.target_message_id,
                    seq: 0, // 删除操作不返回 seq
                    sent_at: operation.timestamp.clone(),
                    timeline: None,
                    status: inner.status,
                }))
            }
            Ok(OperationType::Forward) => {
                // 转发：需要编排（创建新消息 → 存储编排）
                let forward_data = match operation.operation_data {
                    Some(OperationData::Forward(data)) => data,
                    _ => {
                        return Err(Status::invalid_argument(
                            "Forward operation requires ForwardOperationData",
                        ));
                    }
                };

                let forward_req = MessageForwardMessageRequest {
                    message_ids: forward_data.message_ids,
                    target_conversation_id: forward_data.target_conversation_id,
                    reason: forward_data.reason,
                    merge_forward: forward_data.merge_forward,
                    context: request.context.clone(),
                    tenant: request.tenant.clone(),
                };

                let resp = self.forward_message(Request::new(forward_req)).await?;
                let inner = resp.into_inner();

                // 使用第一条转发的消息ID作为返回
                let message_id = inner
                    .forwarded_message_ids
                    .first()
                    .cloned()
                    .unwrap_or_else(|| operation.target_message_id.clone());

                Ok(Response::new(SendMessageResponse {
                    success: inner.success,
                    server_msg_id: message_id,
                    seq: 0, // 转发操作不返回 seq
                    sent_at: operation.timestamp.clone(),
                    timeline: None,
                    status: inner.status,
                }))
            }
            Ok(OperationType::Reply) => {
                // 回复：需要编排（创建新消息 → 存储编排）
                let reply_data = match operation.operation_data {
                    Some(OperationData::Reply(data)) => data,
                    _ => {
                        return Err(Status::invalid_argument(
                            "Reply operation requires ReplyOperationData",
                        ));
                    }
                };

                // 构建回复消息
                let reply_content = reply_data.reply_content.ok_or_else(|| {
                    Status::invalid_argument("Reply operation requires reply_content")
                })?;
                let reply_message = flare_proto::Message {
                    server_id: uuid::Uuid::new_v4().to_string(),
                    conversation_id: request.conversation_id.clone(),
                    sender_id: operation.operator_id.clone(),
                    receiver_id: String::new(), // 回复消息：receiver_id 由原消息决定
                    channel_id: String::new(),  // 回复消息：channel_id 由原消息决定
                    content: Some(reply_content),
                    timestamp: operation.timestamp.clone(),
                    ..Default::default()
                };

                let reply_req = MessageReplyMessageRequest {
                    conversation_id: request.conversation_id.clone(),
                    reply_to_message_id: reply_data.reply_to_message_id,
                    message: Some(reply_message),
                    sync: false,
                    context: request.context.clone(),
                    tenant: request.tenant.clone(),
                };

                let resp = self.reply_message(Request::new(reply_req)).await?;
                let inner = resp.into_inner();

                Ok(Response::new(SendMessageResponse {
                    success: inner.success,
                    server_msg_id: inner.message_id,
                    seq: 0, // 回复操作不返回 seq（回复消息的 seq 在回复消息本身中）
                    sent_at: operation.timestamp.clone(),
                    timeline: None,
                    status: inner.status,
                }))
            }
            Ok(OperationType::Quote) => {
                // 引用：需要编排（创建新消息 → 存储编排）
                let quote_data = match operation.operation_data {
                    Some(OperationData::Quote(data)) => data,
                    _ => {
                        return Err(Status::invalid_argument(
                            "Quote operation requires QuoteOperationData",
                        ));
                    }
                };

                // 使用传入的 message 作为引用消息（已经包含引用内容）
                let quote_req = MessageQuoteMessageRequest {
                    conversation_id: request.conversation_id.clone(),
                    quoted_message_id: quote_data.quoted_message_id,
                    preview_text: quote_data.preview_text,
                    quote_range: None, // quote_data.quote_range 字段在新版 QuoteOperationData 中已移除
                    message: Some(message.clone()),
                    sync: false,
                    context: request.context.clone(),
                    tenant: request.tenant.clone(),
                };

                let resp = self.quote_message(Request::new(quote_req)).await?;
                let inner = resp.into_inner();

                Ok(Response::new(SendMessageResponse {
                    success: inner.success,
                    server_msg_id: inner.message_id,
                    seq: 0, // 引用操作不返回 seq（引用消息的 seq 在引用消息本身中）
                    sent_at: operation.timestamp.clone(),
                    timeline: None,
                    status: inner.status,
                }))
            }
            Ok(OperationType::ThreadReply) => {
                // 话题回复：需要编排（创建新消息 → 存储编排）
                let thread_data = match operation.operation_data {
                    Some(OperationData::Thread(data)) => data,
                    _ => {
                        return Err(Status::invalid_argument(
                            "ThreadReply operation requires ThreadOperationData",
                        ));
                    }
                };

                // 构建话题回复消息
                let thread_reply_content = thread_data.reply_content.ok_or_else(|| {
                    Status::invalid_argument("ThreadReply operation requires reply_content")
                })?;
                let thread_message = flare_proto::Message {
                    server_id: uuid::Uuid::new_v4().to_string(),
                    conversation_id: request.conversation_id.clone(),
                    sender_id: operation.operator_id.clone(),
                    receiver_id: String::new(), // 话题回复：receiver_id 由话题决定
                    channel_id: String::new(),  // 话题回复：channel_id 由话题决定
                    content: Some(thread_reply_content),
                    timestamp: operation.timestamp.clone(),
                    ..Default::default()
                };

                let thread_req = MessageAddThreadReplyRequest {
                    conversation_id: request.conversation_id.clone(),
                    thread_id: thread_data.thread_id,
                    message: Some(thread_message),
                    sync: false,
                    context: request.context.clone(),
                    tenant: request.tenant.clone(),
                };

                let resp = self.add_thread_reply(Request::new(thread_req)).await?;
                let inner = resp.into_inner();

                Ok(Response::new(SendMessageResponse {
                    success: inner.success,
                    server_msg_id: inner.message_id,
                    seq: 0, // 线程回复操作不返回 seq（线程回复消息的 seq 在回复消息本身中）
                    sent_at: operation.timestamp.clone(),
                    timeline: None,
                    status: inner.status,
                }))
            }
            _ => Err(Status::invalid_argument("Not a complex operation")),
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
                conversation_id: send_req.conversation_id.clone(),
                message: send_req.message,
                sync: send_req.sync,
                context: send_req.context,
                tenant: send_req.tenant,
                tags: std::collections::HashMap::new(),
            };

            match self
                .command_handler
                .handle_store_message(StoreMessageCommand {
                    request: store_request,
                })
                .await
            {
                Ok((message_id, _seq)) => {
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
        if req.conversation_id.is_empty() {
            return Err(Status::invalid_argument("conversation_id is required"));
        }

        let mut message = req
            .message
            .ok_or_else(|| Status::invalid_argument("message is required"))?;

        if req.system_message_type.is_empty() {
            return Err(Status::invalid_argument("system_message_type is required"));
        }

        // 构建 StoreMessageRequest，添加系统消息类型标签
        let mut tags = std::collections::HashMap::new();
        tags.insert(
            "system_message_type".to_string(),
            req.system_message_type.clone(),
        );
        tags.insert("is_system_message".to_string(), "true".to_string());
        // 确保消息类型标记为系统消息
        message.extra.insert(
            "system_message_type".to_string(),
            req.system_message_type.clone(),
        );
        message
            .extra
            .insert("sender_type".to_string(), "system".to_string());

        let store_request = StoreMessageRequest {
            conversation_id: req.conversation_id.clone(),
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
            Ok((message_id, _seq)) => {
                info!(
                    message_id = %message_id,
                    conversation_id = %req.conversation_id,
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
                    conversation_id = %req.conversation_id,
                    system_message_type = %req.system_message_type,
                    "Failed to send system message"
                );
                Err(Status::internal(err.to_string()))
            }
        }
    }

    /// 处理 QueryMessages 请求 - 使用 QueryHandler
    #[instrument(skip(self, request))]
    pub async fn query_messages(
        &self,
        request: Request<MessageQueryMessagesRequest>,
    ) -> Result<Response<MessageQueryMessagesResponse>, Status> {
        let req = request.into_inner();

        // 构建查询对象
        let query = crate::application::queries::QueryMessagesQuery {
            conversation_id: req.conversation_id.clone(),
            limit: Some(req.limit),
            cursor: if req.cursor.is_empty() {
                None
            } else {
                Some(req.cursor.clone())
            },
            start_time: if req.start_time == 0 {
                None
            } else {
                Some(req.start_time)
            },
            end_time: if req.end_time == 0 {
                None
            } else {
                Some(req.end_time)
            },
        };

        // 调用查询处理器
        let result = self
            .query_handler
            .query_messages_with_pagination(query)
            .await
            .map_err(|err| {
                error!(error = %err, "Failed to query messages");
                Status::internal(format!("Query messages failed: {}", err))
            })?;

        // 构建响应
        let pagination = if let Some(mut pagination) = req.pagination {
            pagination.has_more = result.has_more;
            pagination.cursor = result.next_cursor.clone();
            Some(pagination)
        } else {
            None
        };

        Ok(Response::new(MessageQueryMessagesResponse {
            messages: result.messages,
            next_cursor: result.next_cursor,
            has_more: result.has_more,
            pagination,
            status: Some(flare_proto::common::RpcStatus {
                code: flare_proto::common::ErrorCode::Ok as i32,
                message: "Success".to_string(),
                details: vec![],
                context: None,
            }),
        }))
    }

    /// 处理 SearchMessages 请求 - 使用 QueryHandler
    #[instrument(skip(self, request))]
    pub async fn search_messages(
        &self,
        request: Request<MessageSearchMessagesRequest>,
    ) -> Result<Response<MessageSearchMessagesResponse>, Status> {
        let req = request.into_inner();

        // 构建搜索查询对象
        let query = crate::application::queries::SearchMessagesQuery {
            conversation_id: None,       // SearchMessagesRequest中没有conversation_id字段
            keyword: String::new(), // SearchMessagesRequest中没有keyword字段，应在filters中处理
            limit: req.pagination.as_ref().map(|p| p.limit),
            cursor: req.pagination.as_ref().and_then(|p| {
                if !p.cursor.is_empty() {
                    Some(p.cursor.clone())
                } else {
                    None
                }
            }),
        };

        // 调用查询处理器
        let messages = self
            .query_handler
            .search_messages(query)
            .await
            .map_err(|err| {
                error!(error = %err, "Failed to search messages");
                Status::internal(format!("Search messages failed: {}", err))
            })?;

        // 构建响应
        Ok(Response::new(MessageSearchMessagesResponse {
            messages,
            pagination: req.pagination,
            status: Some(flare_proto::common::RpcStatus {
                code: flare_proto::common::ErrorCode::Ok as i32,
                message: "Success".to_string(),
                details: vec![],
                context: None,
            }),
        }))
    }

    /// 处理 GetMessage 请求 - 使用 QueryHandler
    #[instrument(skip(self, request))]
    pub async fn get_message(
        &self,
        request: Request<MessageGetMessageRequest>,
    ) -> Result<Response<MessageGetMessageResponse>, Status> {
        let req = request.into_inner();

        // 构建查询对象
        let query = crate::application::queries::QueryMessageQuery {
            message_id: req.message_id.clone(),
            conversation_id: String::new(), // GetMessageRequest中没有conversation_id字段
        };

        // 调用查询处理器
        let message = self
            .query_handler
            .query_message(query)
            .await
            .map_err(|err| {
                error!(error = %err, "Failed to get message");
                Status::internal(format!("Get message failed: {}", err))
            })?;

        // 构建响应
        Ok(Response::new(MessageGetMessageResponse {
            message: Some(message),
            status: Some(flare_proto::common::RpcStatus {
                code: flare_proto::common::ErrorCode::Ok as i32,
                message: "Success".to_string(),
                details: vec![],
                context: None,
            }),
        }))
    }

    /// 处理 RecallMessage 请求 - 转发到 StorageReaderService
    #[instrument(skip(self, request))]
    pub async fn recall_message(
        &self,
        request: Request<MessageRecallMessageRequest>,
    ) -> Result<Response<MessageRecallMessageResponse>, Status> {
        let req = request.into_inner();

        let client = self
            .reader_client
            .as_ref()
            .ok_or_else(|| Status::failed_precondition("Storage Reader not configured"))?;

        let storage_req = RecallMessageRequest {
            message_id: req.message_id.clone(),
            operator_id: req.operator_id.clone(),
            reason: req.reason.clone(),
            recall_time_limit_seconds: req.recall_time_limit_seconds,
            context: req.context.clone(),
            tenant: req.tenant.clone(),
        };

        let storage_resp = client
            .clone()
            .recall_message(Request::new(storage_req.clone()))
            .await
            .map_err(|err| {
                error!(error = ?err, "Failed to recall message from reader");
                Status::internal("recall message from reader failed")
            })?
            .into_inner();

        // 如果撤回成功，推送通知给其他客户端
        if storage_resp.success {
            // 获取原消息以确定接收者
            let get_req = GetMessageRequest {
                context: req.context.clone(),
                tenant: req.tenant.clone(),
                message_id: req.message_id.clone(),
            };

            if let Ok(current) = client.clone().get_message(Request::new(get_req)).await {
                if let Some(original_message) = current.into_inner().message {
                    // 确定接收者（优先使用 receiver_id 和 channel_id）
                    let mut user_ids = Vec::new();

                    // 单聊：使用 receiver_id
                    if original_message.conversation_type
                        == flare_proto::common::ConversationType::Single as i32
                    {
                        if !original_message.receiver_id.is_empty() {
                            user_ids.push(original_message.receiver_id.clone());
                        }
                    }
                    // 群聊/频道：user_ids 留空，推送服务会使用 channel_id 或 conversation_id 查询成员

                    // 构建撤回后的消息（用于推送通知）
                    let mut recalled_message = original_message.clone();
                    recalled_message.status = flare_proto::common::MessageStatus::Recalled as i32;
                    recalled_message.is_recalled = true;
                    recalled_message.recalled_at = storage_resp.recalled_at.clone();
                    if !req.reason.is_empty() {
                        recalled_message.recall_reason = req.reason.clone();
                    }

                    // 构建推送请求并发布到 Kafka
                    if !user_ids.is_empty() {
                        let push_request = flare_proto::push::PushMessageRequest {
                            user_ids,
                            message: Some(recalled_message),
                            options: Some(flare_proto::push::PushOptions {
                                require_online: false,
                                persist_if_offline: true,
                                priority: 5,
                                metadata: std::collections::HashMap::new(),
                                channel: String::new(),
                                mute_when_quiet: false,
                            }),
                            context: req.context.clone(),
                            tenant: req.tenant.clone(),
                            template_id: String::new(),
                            template_data: std::collections::HashMap::new(),
                        };

                        // 发布推送请求到 Kafka
                        if let Err(e) = self.publisher.publish_push(push_request).await {
                            error!(
                                error = %e,
                                message_id = %req.message_id,
                                "Failed to publish recall notification to push queue"
                            );
                            // 不返回错误，因为数据库更新已成功，推送失败不应该影响撤回操作
                        } else {
                            info!(
                                message_id = %req.message_id,
                                "Recall notification published to push queue"
                            );
                        }
                    } else {
                        warn!(
                            message_id = %req.message_id,
                            conversation_id = %original_message.conversation_id,
                            "No receivers found for recall notification, skipping push"
                        );
                    }
                }
            }
        }

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

        let client = self
            .reader_client
            .as_ref()
            .ok_or_else(|| Status::failed_precondition("Storage Reader not configured"))?;

        let storage_req = DeleteMessageRequest {
            conversation_id: req.conversation_id,
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

        let client = self
            .reader_client
            .as_ref()
            .ok_or_else(|| Status::failed_precondition("Storage Reader not configured"))?;

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

    #[instrument(skip(self, request))]
    pub async fn edit_message(
        &self,
        request: Request<MessageEditMessageRequest>,
    ) -> Result<Response<MessageEditMessageResponse>, Status> {
        let req = request.into_inner();

        let client = self
            .reader_client
            .as_ref()
            .ok_or_else(|| Status::failed_precondition("Storage Reader not configured"))?;

        // 获取原消息
        let get_req = GetMessageRequest {
            context: req.context.clone(),
            tenant: req.tenant.clone(),
            message_id: req.message_id.clone(),
        };
        let current = client
            .clone()
            .get_message(Request::new(get_req))
            .await
            .map_err(|err| {
                error!(error = ?err, "Failed to get message for edit");
                Status::internal("get message failed")
            })?
            .into_inner();

        let original_message = current
            .message
            .ok_or_else(|| Status::not_found("Message not found"))?;

        // 验证操作者权限（只有发送者可以编辑）
        let operator_id = req
            .context
            .as_ref()
            .and_then(|c| c.actor.as_ref())
            .map(|a| a.actor_id.as_str())
            .unwrap_or("");

        if original_message.sender_id != operator_id {
            return Err(Status::permission_denied(
                "Only message sender can edit message",
            ));
        }

        // 构建编辑后的消息内容
        let mut attrs = std::collections::HashMap::new();

        // 记录编辑信息
        let now = chrono::Utc::now();
        attrs.insert("edited_at".to_string(), now.to_rfc3339());
        attrs.insert("edit_version".to_string(), req.edit_version.to_string());
        if req.show_edited_mark {
            attrs.insert("show_edited_mark".to_string(), "true".to_string());
        }
        if !req.reason.is_empty() {
            attrs.insert("edit_reason".to_string(), req.reason);
        }

        // 更新消息内容
        // 注意：由于 Storage Reader 的限制，编辑后的内容通过推送通知同步
        // attributes 中记录编辑元数据，实际内容更新通过推送通知传递
        let new_content_clone = req.new_content.clone();
        if let Some(new_content) = &req.new_content {
            // 记录编辑后的内容类型（用于客户端识别）
            let content_type = match &new_content.content {
                Some(flare_proto::common::message_content::Content::Text(_)) => "text",
                Some(flare_proto::common::message_content::Content::Image(_)) => "image",
                Some(flare_proto::common::message_content::Content::Video(_)) => "video",
                Some(flare_proto::common::message_content::Content::Audio(_)) => "audio",
                Some(flare_proto::common::message_content::Content::File(_)) => "file",
                Some(flare_proto::common::message_content::Content::Location(_)) => "location",
                Some(flare_proto::common::message_content::Content::Card(_)) => "card",
                Some(flare_proto::common::message_content::Content::Notification(_)) => "notification",
                Some(flare_proto::common::message_content::Content::Custom(_)) => "custom",
                Some(flare_proto::common::message_content::Content::Forward(_)) => "forward",
                Some(flare_proto::common::message_content::Content::Typing(_)) => "typing",
                Some(flare_proto::common::message_content::Content::Quote(_)) => "quote",
                Some(flare_proto::common::message_content::Content::LinkCard(_)) => "link_card",
                Some(flare_proto::common::message_content::Content::SystemEvent(_)) => "system_event",
                None => "unknown",
            };
            attrs.insert("edited_content_type".to_string(), content_type.to_string());
        }

        let storage_req = SetMessageAttributesRequest {
            context: req.context.clone(),
            tenant: req.tenant.clone(),
            message_id: req.message_id.clone(),
            attributes: attrs.clone(), // 克隆 attrs，因为后面还需要使用
            tags: vec![],
        };

        let _ = client
            .clone()
            .set_message_attributes(Request::new(storage_req))
            .await
            .map_err(|err| {
                error!(error = ?err, "Failed to set message attributes for edit");
                Status::internal("edit message failed")
            })?;

        // 构建编辑后的消息（用于推送通知）
        let mut edited_message = original_message.clone();
        edited_message.attributes.extend(attrs); // 现在可以移动 attrs，因为已经克隆过了
        if let Some(new_content) = new_content_clone {
            edited_message.content = Some(new_content); // 使用克隆的 new_content
        }
        edited_message.status = flare_proto::common::MessageStatus::Sent as i32; // 保持为 Sent 状态

        // 确定接收者（优先使用 receiver_id 和 channel_id）
        let mut user_ids = Vec::new();

        // 单聊：使用 receiver_id
        if original_message.conversation_type == flare_proto::common::ConversationType::Single as i32 {
            if !original_message.receiver_id.is_empty() {
                user_ids.push(original_message.receiver_id.clone());
            }
        }
        // 群聊/频道：user_ids 留空，推送服务会使用 channel_id 或 conversation_id 查询成员

        // 构建推送请求并发布到 Kafka
        if !user_ids.is_empty() {
            let push_request = flare_proto::push::PushMessageRequest {
                user_ids,
                message: Some(edited_message),
                options: Some(flare_proto::push::PushOptions {
                    require_online: false,
                    persist_if_offline: true,
                    priority: 5,
                    metadata: std::collections::HashMap::new(),
                    channel: String::new(),
                    mute_when_quiet: false,
                }),
                context: req.context.clone(),
                tenant: req.tenant.clone(),
                template_id: String::new(),
                template_data: std::collections::HashMap::new(),
            };

            // 发布推送请求到 Kafka
            if let Err(e) = self.publisher.publish_push(push_request).await {
                error!(
                    error = %e,
                    message_id = %req.message_id,
                    "Failed to publish edit notification to push queue"
                );
                // 不返回错误，因为数据库更新已成功，推送失败不应该影响编辑操作
            } else {
                info!(
                    message_id = %req.message_id,
                    "Edit notification published to push queue"
                );
            }
        } else {
            warn!(
                message_id = %req.message_id,
                conversation_id = %original_message.conversation_id,
                "No receivers found for edit notification, skipping push"
            );
        }

        Ok(Response::new(MessageEditMessageResponse {
            success: true,
            error_message: String::new(),
            message_id: req.message_id,
            edit_version: req.edit_version,
            edited_at: Some(prost_types::Timestamp {
                seconds: now.timestamp(),
                nanos: now.timestamp_subsec_nanos() as i32,
            }),
            status: Some(ok_status()),
        }))
    }

    #[instrument(skip(self, request))]
    pub async fn add_reaction(
        &self,
        request: Request<MessageAddReactionRequest>,
    ) -> Result<Response<MessageAddReactionResponse>, Status> {
        let req = request.into_inner();

        let client = self
            .reader_client
            .as_ref()
            .ok_or_else(|| Status::failed_precondition("Storage Reader not configured"))?;

        let get_req = GetMessageRequest {
            context: req.context.clone(),
            tenant: req.tenant.clone(),
            message_id: req.message_id.clone(),
        };
        let current = client
            .clone()
            .get_message(Request::new(get_req))
            .await
            .map_err(|err| {
                error!(error = ?err, "Failed to get message for add_reaction");
                Status::internal("get message failed")
            })?
            .into_inner();

        let mut count = 0i32;
        if let Some(msg) = current.message {
            let key = format!("reaction:{}:count", req.emoji);
            if let Some(v) = msg.attributes.get(&key) {
                if let Ok(n) = v.parse::<i32>() {
                    count = n;
                }
            }
        }
        let new_count = count.saturating_add(1);

        let mut attrs = std::collections::HashMap::new();
        attrs.insert(
            format!("reaction:{}:count", req.emoji),
            new_count.to_string(),
        );
        attrs.insert(
            format!("reaction:{}:last_by", req.emoji),
            req.user_id.clone(),
        );

        let set_req = SetMessageAttributesRequest {
            context: req.context,
            tenant: req.tenant,
            message_id: req.message_id,
            attributes: attrs,
            tags: vec![],
        };
        let _ = client
            .clone()
            .set_message_attributes(Request::new(set_req))
            .await
            .map_err(|err| {
                error!(error = ?err, "Failed to set attributes for add_reaction");
                Status::internal("add reaction failed")
            })?;

        Ok(Response::new(MessageAddReactionResponse {
            success: true,
            error_message: String::new(),
            new_count,
            status: Some(ok_status()),
        }))
    }

    #[instrument(skip(self, request))]
    pub async fn remove_reaction(
        &self,
        request: Request<MessageRemoveReactionRequest>,
    ) -> Result<Response<MessageRemoveReactionResponse>, Status> {
        let req = request.into_inner();

        let client = self
            .reader_client
            .as_ref()
            .ok_or_else(|| Status::failed_precondition("Storage Reader not configured"))?;

        let get_req = GetMessageRequest {
            context: req.context.clone(),
            tenant: req.tenant.clone(),
            message_id: req.message_id.clone(),
        };
        let current = client
            .clone()
            .get_message(Request::new(get_req))
            .await
            .map_err(|err| {
                error!(error = ?err, "Failed to get message for remove_reaction");
                Status::internal("get message failed")
            })?
            .into_inner();

        let mut count = 0i32;
        if let Some(msg) = current.message {
            let key = format!("reaction:{}:count", req.emoji);
            if let Some(v) = msg.attributes.get(&key) {
                if let Ok(n) = v.parse::<i32>() {
                    count = n;
                }
            }
        }
        let new_count = (count - 1).max(0);

        let mut attrs = std::collections::HashMap::new();
        attrs.insert(
            format!("reaction:{}:count", req.emoji),
            new_count.to_string(),
        );
        attrs.insert(
            format!("reaction:{}:last_by", req.emoji),
            req.user_id.clone(),
        );

        let set_req = SetMessageAttributesRequest {
            context: req.context,
            tenant: req.tenant,
            message_id: req.message_id,
            attributes: attrs,
            tags: vec![],
        };
        let _ = client
            .clone()
            .set_message_attributes(Request::new(set_req))
            .await
            .map_err(|err| {
                error!(error = ?err, "Failed to set attributes for remove_reaction");
                Status::internal("remove reaction failed")
            })?;

        Ok(Response::new(MessageRemoveReactionResponse {
            success: true,
            error_message: String::new(),
            new_count,
            status: Some(ok_status()),
        }))
    }

    #[instrument(skip(self, request))]
    pub async fn reply_message(
        &self,
        request: Request<MessageReplyMessageRequest>,
    ) -> Result<Response<MessageReplyMessageResponse>, Status> {
        let req = request.into_inner();

        let mut message = req
            .message
            .ok_or_else(|| Status::invalid_argument("message required"))?;
        message
            .attributes
            .insert("reply_to".to_string(), req.reply_to_message_id.clone());

        let send_req = SendMessageRequest {
            conversation_id: req.conversation_id,
            message: Some(message),
            sync: req.sync,
            context: req.context.clone(),
            tenant: req.tenant.clone(),
        };

        let send_resp = self
            .send_normal_message_internal(&send_req.message.as_ref().unwrap(), &send_req)
            .await?
            .into_inner();

        let client = self
            .reader_client
            .as_ref()
            .ok_or_else(|| Status::failed_precondition("Storage Reader not configured"))?;

        let get_req = flare_proto::storage::GetMessageRequest {
            context: req.context.clone(),
            tenant: req.tenant.clone(),
            message_id: req.reply_to_message_id.clone(),
        };
        let current = client
            .clone()
            .get_message(Request::new(get_req))
            .await
            .map_err(|err| {
                error!(error = ?err, "Failed to get original message for reply");
                Status::internal("reply get original failed")
            })?
            .into_inner();

        let mut count = 0i32;
        if let Some(msg) = current.message {
            if let Some(v) = msg.attributes.get("reply_count") {
                if let Ok(n) = v.parse::<i32>() {
                    count = n;
                }
            }
        }
        let new_count = count.saturating_add(1);

        let mut attrs = std::collections::HashMap::new();
        attrs.insert("reply_count".to_string(), new_count.to_string());
        if let Some(actor) = req.context.as_ref().and_then(|c| c.actor.as_ref()) {
            attrs.insert("reply_last_by".to_string(), actor.actor_id.clone());
        }

        let set_req = flare_proto::storage::SetMessageAttributesRequest {
            context: req.context,
            tenant: req.tenant,
            message_id: req.reply_to_message_id,
            attributes: attrs,
            tags: vec![],
        };
        let _ = client
            .clone()
            .set_message_attributes(Request::new(set_req))
            .await
            .map_err(|err| {
                error!(error = ?err, "Failed to set attributes for reply count");
                Status::internal("reply set attributes failed")
            })?;

        Ok(Response::new(MessageReplyMessageResponse {
            success: send_resp.success,
            message_id: send_resp.server_msg_id,
            error_message: String::new(),
            status: Some(ok_status()),
        }))
    }

    #[instrument(skip(self, request))]
    pub async fn add_thread_reply(
        &self,
        request: Request<MessageAddThreadReplyRequest>,
    ) -> Result<Response<MessageAddThreadReplyResponse>, Status> {
        let req = request.into_inner();

        let mut message = req
            .message
            .ok_or_else(|| Status::invalid_argument("message required"))?;
        message
            .attributes
            .insert("thread_id".to_string(), req.thread_id.clone());

        let send_req = SendMessageRequest {
            conversation_id: req.conversation_id,
            message: Some(message),
            sync: req.sync,
            context: req.context.clone(),
            tenant: req.tenant.clone(),
        };

        let send_resp = self
            .send_normal_message_internal(&send_req.message.as_ref().unwrap(), &send_req)
            .await?
            .into_inner();

        let client = self
            .reader_client
            .as_ref()
            .ok_or_else(|| Status::failed_precondition("Storage Reader not configured"))?;

        let get_req = flare_proto::storage::GetMessageRequest {
            context: req.context.clone(),
            tenant: req.tenant.clone(),
            message_id: req.thread_id.clone(),
        };
        let current = client
            .clone()
            .get_message(Request::new(get_req))
            .await
            .map_err(|err| {
                error!(error = ?err, "Failed to get thread head message");
                Status::internal("thread get head failed")
            })?
            .into_inner();

        let mut count = 0i32;
        if let Some(msg) = current.message {
            if let Some(v) = msg.attributes.get("thread_reply_count") {
                if let Ok(n) = v.parse::<i32>() {
                    count = n;
                }
            }
        }
        let new_count = count.saturating_add(1);

        let mut attrs = std::collections::HashMap::new();
        attrs.insert("thread_reply_count".to_string(), new_count.to_string());
        if let Some(actor) = req.context.as_ref().and_then(|c| c.actor.as_ref()) {
            attrs.insert("thread_last_by".to_string(), actor.actor_id.clone());
        }

        let set_req = flare_proto::storage::SetMessageAttributesRequest {
            context: req.context,
            tenant: req.tenant,
            message_id: req.thread_id,
            attributes: attrs,
            tags: vec![],
        };
        let _ = client
            .clone()
            .set_message_attributes(Request::new(set_req))
            .await
            .map_err(|err| {
                error!(error = ?err, "Failed to set attributes for thread reply count");
                Status::internal("thread set attributes failed")
            })?;

        Ok(Response::new(MessageAddThreadReplyResponse {
            success: send_resp.success,
            message_id: send_resp.server_msg_id,
            error_message: String::new(),
            status: Some(ok_status()),
        }))
    }

    /// 处理 ForwardMessage 请求 - 转发消息到目标会话
    #[instrument(skip(self, request))]
    pub async fn forward_message(
        &self,
        request: Request<MessageForwardMessageRequest>,
    ) -> Result<Response<MessageForwardMessageResponse>, Status> {
        let req = request.into_inner();

        if req.message_ids.is_empty() {
            return Err(Status::invalid_argument("message_ids cannot be empty"));
        }

        if req.message_ids.len() > 100 {
            return Err(Status::invalid_argument("message_ids cannot exceed 100"));
        }

        let client = self
            .reader_client
            .as_ref()
            .ok_or_else(|| Status::failed_precondition("Storage Reader not configured"))?;

        // 获取被转发的消息
        let mut messages_to_forward = Vec::new();
        for message_id in &req.message_ids {
            let get_req = GetMessageRequest {
                context: req.context.clone(),
                tenant: req.tenant.clone(),
                message_id: message_id.clone(),
            };
            let resp = client
                .clone()
                .get_message(Request::new(get_req))
                .await
                .map_err(|err| {
                    error!(error = ?err, message_id = %message_id, "Failed to get message for forward");
                    Status::internal("get message failed")
                })?;

            if let Some(msg) = resp.into_inner().message {
                messages_to_forward.push(msg);
            }
        }

        if messages_to_forward.is_empty() {
            return Err(Status::not_found("No messages found to forward"));
        }

        // 构建转发消息
        let mut forwarded_message_ids = Vec::new();

        if req.merge_forward && messages_to_forward.len() > 1 {
            // 合并转发：创建一条包含所有消息的转发消息
            let forward_content = flare_proto::common::ForwardContent {
                message_ids: req.message_ids.clone(),
                forward_reason: req.reason.clone(),
                // 注意：metadata 字段在新版 ForwardContent 中已移除
                // metadata: std::collections::HashMap::new(),
            };

            let forward_message = flare_proto::common::Message {
                conversation_id: req.target_conversation_id.clone(),
                receiver_id: String::new(), // 转发消息：receiver_id 由目标会话决定
                channel_id: String::new(),  // 转发消息：channel_id 由目标会话决定
                message_type: flare_proto::common::MessageType::Forward as i32,
                content: Some(flare_proto::common::MessageContent {
                    content: Some(flare_proto::common::message_content::Content::Forward(
                        forward_content,
                    )),
                    // 注意：extensions 字段是必需的，类型为 Vec<prost_types::Any>
                    extensions: vec![],
                }),
                ..Default::default()
            };

            // 设置转发信息（注意：forward_info 字段在新版 Message 中已移除）
            // forward_message.forward_info = Some(flare_proto::common::ForwardContent {
            //     message_ids: req.message_ids.clone(),
            //     forward_reason: req.reason,
            //     metadata: std::collections::HashMap::new(),
            // });

            let send_req = SendMessageRequest {
                conversation_id: req.target_conversation_id,
                message: Some(forward_message),
                sync: false,
                context: req.context.clone(),
                tenant: req.tenant.clone(),
                // 注意：payload 字段在新版 SendMessageRequest 中已移除
            };

            let send_resp = self
                .send_normal_message_internal(&send_req.message.as_ref().unwrap(), &send_req)
                .await?
                .into_inner();

            if send_resp.success {
                forwarded_message_ids.push(send_resp.server_msg_id);
            }
        } else {
            // 分别转发：每条消息单独转发
            for msg in messages_to_forward {
                let mut forward_message = msg.clone();
                forward_message.conversation_id = req.target_conversation_id.clone();
                forward_message.message_type = flare_proto::common::MessageType::Forward as i32;

                // 设置转发信息（注意：forward_info 字段在新版 Message 中已移除）
                // forward_message.forward_info = Some(flare_proto::common::ForwardContent {
                //     message_ids: vec![msg.server_id.clone()],
                //     forward_reason: req.reason.clone(),
                //     metadata: std::collections::HashMap::new(),
                // });

                let send_req = SendMessageRequest {
                    conversation_id: req.target_conversation_id.clone(),
                    message: Some(forward_message),
                    sync: false,
                    context: req.context.clone(),
                    tenant: req.tenant.clone(),
                    // 注意：payload 字段在新版 SendMessageRequest 中已移除
                };

                let send_resp = self
                    .send_message(Request::new(send_req))
                    .await?
                    .into_inner();

                if send_resp.success {
                    forwarded_message_ids.push(send_resp.server_msg_id);
                }
            }
        }

        Ok(Response::new(MessageForwardMessageResponse {
            success: !forwarded_message_ids.is_empty(),
            error_message: if forwarded_message_ids.is_empty() {
                "Failed to forward messages".to_string()
            } else {
                String::new()
            },
            forwarded_message_ids,
            status: Some(ok_status()),
        }))
    }

    /// 处理 QuoteMessage 请求 - 引用消息
    #[instrument(skip(self, request))]
    pub async fn quote_message(
        &self,
        request: Request<MessageQuoteMessageRequest>,
    ) -> Result<Response<MessageQuoteMessageResponse>, Status> {
        let req = request.into_inner();

        let client = self
            .reader_client
            .as_ref()
            .ok_or_else(|| Status::failed_precondition("Storage Reader not configured"))?;

        // 获取被引用的消息
        let get_req = GetMessageRequest {
            context: req.context.clone(),
            tenant: req.tenant.clone(),
            message_id: req.quoted_message_id.clone(),
        };
        let quoted_msg = client
            .clone()
            .get_message(Request::new(get_req))
            .await
            .map_err(|err| {
                error!(error = ?err, "Failed to get quoted message");
                Status::internal("get quoted message failed")
            })?
            .into_inner()
            .message
            .ok_or_else(|| Status::not_found("Quoted message not found"))?;

        // 构建引用消息内容
        // 注意：quoted_content在proto中定义为optional MessageContent，Rust生成的是Option<Box<MessageContent>>
        // quoted_msg.content是Option<MessageContent>，需要转换为Option<Box<MessageContent>>
        let quoted_content = quoted_msg.content.clone().map(|c| Box::new(c));

        let quote_content = flare_proto::common::QuoteContent {
            quoted_message_id: req.quoted_message_id.clone(),
            quoted_sender_id: quoted_msg.sender_id.clone(),
            quoted_text_preview: req.preview_text.clone(),
            quoted_content,
            // 注意：quote_range 字段在新版 QuoteContent 中已移除
            // quote_range: req.quote_range.clone(),
        };

        let mut message = req
            .message
            .ok_or_else(|| Status::invalid_argument("message required"))?;
        message.content = Some(flare_proto::common::MessageContent {
            content: Some(flare_proto::common::message_content::Content::Quote(
                Box::new(quote_content),
            )),
            // 注意：extensions 字段是必需的
            extensions: vec![],
        });
        message.message_type = flare_proto::common::MessageType::Quote as i32;

        // 设置引用关系
        message
            .attributes
            .insert("quote_to".to_string(), req.quoted_message_id.clone());

        let send_req = SendMessageRequest {
            conversation_id: req.conversation_id,
            message: Some(message),
            sync: req.sync,
            context: req.context.clone(),
            tenant: req.tenant.clone(),
        };

        let send_resp = self
            .send_normal_message_internal(&send_req.message.as_ref().unwrap(), &send_req)
            .await?
            .into_inner();

        Ok(Response::new(MessageQuoteMessageResponse {
            success: send_resp.success,
            message_id: send_resp.server_msg_id,
            error_message: String::new(),
            status: Some(ok_status()),
        }))
    }

    /// 处理 PinMessage 请求 - 置顶消息
    #[instrument(skip(self, request))]
    pub async fn pin_message(
        &self,
        request: Request<MessagePinMessageRequest>,
    ) -> Result<Response<MessagePinMessageResponse>, Status> {
        let req = request.into_inner();

        let client = self
            .reader_client
            .as_ref()
            .ok_or_else(|| Status::failed_precondition("Storage Reader not configured"))?;

        let mut attrs = std::collections::HashMap::new();
        attrs.insert("pinned".to_string(), "true".to_string());
        attrs.insert("pinned_by".to_string(), req.operator_id.clone());

        let now = chrono::Utc::now();
        attrs.insert("pinned_at".to_string(), now.to_rfc3339());

        if !req.reason.is_empty() {
            attrs.insert("pin_reason".to_string(), req.reason);
        }

        if let Some(expire_at) = req.expire_at {
            attrs.insert(
                "pin_expire_at".to_string(),
                chrono::DateTime::<chrono::Utc>::from_timestamp(
                    expire_at.seconds,
                    expire_at.nanos as u32,
                )
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_default(),
            );
        }

        let storage_req = SetMessageAttributesRequest {
            context: req.context.clone(),
            tenant: req.tenant.clone(),
            message_id: req.message_id.clone(),
            attributes: attrs,
            tags: vec![],
        };

        let _ = client
            .clone()
            .set_message_attributes(Request::new(storage_req))
            .await
            .map_err(|err| {
                error!(error = ?err, "Failed to pin message");
                Status::internal("pin message failed")
            })?;

        Ok(Response::new(MessagePinMessageResponse {
            success: true,
            error_message: String::new(),
            pinned_at: Some(prost_types::Timestamp {
                seconds: now.timestamp(),
                nanos: now.timestamp_subsec_nanos() as i32,
            }),
            status: Some(ok_status()),
        }))
    }

    /// 处理 UnpinMessage 请求 - 取消置顶
    #[instrument(skip(self, request))]
    pub async fn unpin_message(
        &self,
        request: Request<MessageUnpinMessageRequest>,
    ) -> Result<Response<MessageUnpinMessageResponse>, Status> {
        let req = request.into_inner();

        let client = self
            .reader_client
            .as_ref()
            .ok_or_else(|| Status::failed_precondition("Storage Reader not configured"))?;

        let mut attrs = std::collections::HashMap::new();
        attrs.insert("pinned".to_string(), "false".to_string());
        attrs.insert("unpinned_by".to_string(), req.operator_id.clone());

        let now = chrono::Utc::now();
        attrs.insert("unpinned_at".to_string(), now.to_rfc3339());

        let storage_req = SetMessageAttributesRequest {
            context: req.context,
            tenant: req.tenant,
            message_id: req.message_id,
            attributes: attrs,
            tags: vec![],
        };

        let _ = client
            .clone()
            .set_message_attributes(Request::new(storage_req))
            .await
            .map_err(|err| {
                error!(error = ?err, "Failed to unpin message");
                Status::internal("unpin message failed")
            })?;

        Ok(Response::new(MessageUnpinMessageResponse {
            success: true,
            error_message: String::new(),
            status: Some(ok_status()),
        }))
    }

    /// 处理 FavoriteMessage 请求 - 收藏消息
    #[instrument(skip(self, request))]
    pub async fn favorite_message(
        &self,
        request: Request<MessageFavoriteMessageRequest>,
    ) -> Result<Response<MessageFavoriteMessageResponse>, Status> {
        let req = request.into_inner();

        let client = self
            .reader_client
            .as_ref()
            .ok_or_else(|| Status::failed_precondition("Storage Reader not configured"))?;

        let mut attrs = std::collections::HashMap::new();
        attrs.insert(format!("favorited_by:{}", req.user_id), "true".to_string());

        let now = chrono::Utc::now();
        attrs.insert(format!("favorited_at:{}", req.user_id), now.to_rfc3339());

        if !req.tags.is_empty() {
            attrs.insert(format!("favorite_tags:{}", req.user_id), req.tags.join(","));
        }

        if !req.note.is_empty() {
            attrs.insert(format!("favorite_note:{}", req.user_id), req.note);
        }

        let storage_req = SetMessageAttributesRequest {
            context: req.context.clone(),
            tenant: req.tenant.clone(),
            message_id: req.message_id.clone(),
            attributes: attrs,
            tags: vec![],
        };

        let _ = client
            .clone()
            .set_message_attributes(Request::new(storage_req))
            .await
            .map_err(|err| {
                error!(error = ?err, "Failed to favorite message");
                Status::internal("favorite message failed")
            })?;

        Ok(Response::new(MessageFavoriteMessageResponse {
            success: true,
            error_message: String::new(),
            favorited_at: Some(prost_types::Timestamp {
                seconds: now.timestamp(),
                nanos: now.timestamp_subsec_nanos() as i32,
            }),
            status: Some(ok_status()),
        }))
    }

    /// 处理 UnfavoriteMessage 请求 - 取消收藏
    #[instrument(skip(self, request))]
    pub async fn unfavorite_message(
        &self,
        request: Request<MessageUnfavoriteMessageRequest>,
    ) -> Result<Response<MessageUnfavoriteMessageResponse>, Status> {
        let req = request.into_inner();

        let client = self
            .reader_client
            .as_ref()
            .ok_or_else(|| Status::failed_precondition("Storage Reader not configured"))?;

        let mut attrs = std::collections::HashMap::new();
        attrs.insert(format!("favorited_by:{}", req.user_id), "false".to_string());
        attrs.insert(
            format!("unfavorited_at:{}", req.user_id),
            chrono::Utc::now().to_rfc3339(),
        );

        let storage_req = SetMessageAttributesRequest {
            context: req.context,
            tenant: req.tenant,
            message_id: req.message_id,
            attributes: attrs,
            tags: vec![],
        };

        let _ = client
            .clone()
            .set_message_attributes(Request::new(storage_req))
            .await
            .map_err(|err| {
                error!(error = ?err, "Failed to unfavorite message");
                Status::internal("unfavorite message failed")
            })?;

        Ok(Response::new(MessageUnfavoriteMessageResponse {
            success: true,
            error_message: String::new(),
            status: Some(ok_status()),
        }))
    }

    /// 处理 MarkMessage 请求 - 标记消息
    #[instrument(skip(self, request))]
    pub async fn mark_message(
        &self,
        request: Request<MessageMarkMessageRequest>,
    ) -> Result<Response<MessageMarkMessageResponse>, Status> {
        let req = request.into_inner();

        let client = self
            .reader_client
            .as_ref()
            .ok_or_else(|| Status::failed_precondition("Storage Reader not configured"))?;

        let mut attrs = std::collections::HashMap::new();
        attrs.insert(
            format!("marked_by:{}", req.user_id),
            format!("{}", req.mark_type as i32),
        );

        let now = chrono::Utc::now();
        attrs.insert(format!("marked_at:{}", req.user_id), now.to_rfc3339());

        if !req.color.is_empty() {
            attrs.insert(format!("mark_color:{}", req.user_id), req.color);
        }

        let storage_req = SetMessageAttributesRequest {
            context: req.context.clone(),
            tenant: req.tenant.clone(),
            message_id: req.message_id.clone(),
            attributes: attrs,
            tags: vec![],
        };

        let _ = client
            .clone()
            .set_message_attributes(Request::new(storage_req))
            .await
            .map_err(|err| {
                error!(error = ?err, "Failed to mark message");
                Status::internal("mark message failed")
            })?;

        Ok(Response::new(MessageMarkMessageResponse {
            success: true,
            error_message: String::new(),
            marked_at: Some(prost_types::Timestamp {
                seconds: now.timestamp(),
                nanos: now.timestamp_subsec_nanos() as i32,
            }),
            status: Some(ok_status()),
        }))
    }

    /// 处理 BatchMarkMessageRead 请求 - 批量标记已读
    #[instrument(skip(self, request))]
    pub async fn batch_mark_message_read(
        &self,
        request: Request<MessageBatchMarkMessageReadRequest>,
    ) -> Result<Response<MessageBatchMarkMessageReadResponse>, Status> {
        let req = request.into_inner();

        let client = self
            .reader_client
            .as_ref()
            .ok_or_else(|| Status::failed_precondition("Storage Reader not configured"))?;

        let read_at = req
            .read_at
            .map(|ts| {
                chrono::DateTime::<chrono::Utc>::from_timestamp(ts.seconds, ts.nanos as u32)
                    .unwrap_or_else(|| chrono::Utc::now())
            })
            .unwrap_or_else(|| chrono::Utc::now());

        let mut read_count = 0i32;

        if req.message_ids.is_empty() {
            // 如果没有指定消息ID，标记会话中所有未读消息为已读
            // 这里需要查询会话的未读消息列表（简化实现，实际应该查询未读消息）
            // 暂时返回成功，实际实现需要查询未读消息列表
            read_count = 0;
        } else {
            // 批量标记指定的消息为已读
            for message_id in &req.message_ids {
                let mark_req = MarkMessageReadRequest {
                    message_id: message_id.clone(),
                    user_id: req.user_id.clone(),
                    context: req.context.clone(),
                    tenant: req.tenant.clone(),
                };

                match client
                    .clone()
                    .mark_message_read(Request::new(mark_req))
                    .await
                {
                    Ok(resp) => {
                        if resp.into_inner().success {
                            read_count += 1;
                        }
                    }
                    Err(err) => {
                        error!(error = ?err, message_id = %message_id, "Failed to mark message read in batch");
                        // 继续处理其他消息
                    }
                }
            }
        }

        Ok(Response::new(MessageBatchMarkMessageReadResponse {
            success: read_count > 0,
            error_message: if read_count == 0 {
                "No messages marked as read".to_string()
            } else {
                String::new()
            },
            read_count,
            read_at: Some(prost_types::Timestamp {
                seconds: read_at.timestamp(),
                nanos: read_at.timestamp_subsec_nanos() as i32,
            }),
            status: Some(ok_status()),
        }))
    }
}

#[tonic::async_trait]
impl MessageService for MessageGrpcHandler {
    async fn send_message(
        &self,
        request: Request<SendMessageRequest>,
    ) -> Result<Response<SendMessageResponse>, Status> {
        self.send_message_impl(request).await
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

    async fn edit_message(
        &self,
        request: Request<MessageEditMessageRequest>,
    ) -> Result<Response<MessageEditMessageResponse>, Status> {
        self.edit_message(request).await
    }

    async fn add_reaction(
        &self,
        request: Request<MessageAddReactionRequest>,
    ) -> Result<Response<MessageAddReactionResponse>, Status> {
        self.add_reaction(request).await
    }

    async fn remove_reaction(
        &self,
        request: Request<MessageRemoveReactionRequest>,
    ) -> Result<Response<MessageRemoveReactionResponse>, Status> {
        self.remove_reaction(request).await
    }

    async fn reply_message(
        &self,
        request: Request<MessageReplyMessageRequest>,
    ) -> Result<Response<MessageReplyMessageResponse>, Status> {
        self.reply_message(request).await
    }

    async fn add_thread_reply(
        &self,
        request: Request<MessageAddThreadReplyRequest>,
    ) -> Result<Response<MessageAddThreadReplyResponse>, Status> {
        self.add_thread_reply(request).await
    }

    async fn forward_message(
        &self,
        request: Request<MessageForwardMessageRequest>,
    ) -> Result<Response<MessageForwardMessageResponse>, Status> {
        self.forward_message(request).await
    }

    async fn quote_message(
        &self,
        request: Request<MessageQuoteMessageRequest>,
    ) -> Result<Response<MessageQuoteMessageResponse>, Status> {
        self.quote_message(request).await
    }

    async fn pin_message(
        &self,
        request: Request<MessagePinMessageRequest>,
    ) -> Result<Response<MessagePinMessageResponse>, Status> {
        self.pin_message(request).await
    }

    async fn unpin_message(
        &self,
        request: Request<MessageUnpinMessageRequest>,
    ) -> Result<Response<MessageUnpinMessageResponse>, Status> {
        self.unpin_message(request).await
    }

    async fn favorite_message(
        &self,
        request: Request<MessageFavoriteMessageRequest>,
    ) -> Result<Response<MessageFavoriteMessageResponse>, Status> {
        self.favorite_message(request).await
    }

    async fn unfavorite_message(
        &self,
        request: Request<MessageUnfavoriteMessageRequest>,
    ) -> Result<Response<MessageUnfavoriteMessageResponse>, Status> {
        self.unfavorite_message(request).await
    }

    async fn mark_message(
        &self,
        request: Request<MessageMarkMessageRequest>,
    ) -> Result<Response<MessageMarkMessageResponse>, Status> {
        self.mark_message(request).await
    }

    async fn batch_mark_message_read(
        &self,
        request: Request<MessageBatchMarkMessageReadRequest>,
    ) -> Result<Response<MessageBatchMarkMessageReadResponse>, Status> {
        self.batch_mark_message_read(request).await
    }
}
