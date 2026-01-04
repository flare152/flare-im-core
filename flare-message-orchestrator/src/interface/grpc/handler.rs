use std::sync::Arc;

use flare_im_core::error::ok_status;
use flare_proto::message::{
    AddReactionRequest as MessageAddReactionRequest,
    AddReactionResponse as MessageAddReactionResponse,
    BatchMarkMessageReadRequest as MessageBatchMarkMessageReadRequest,
    BatchMarkMessageReadResponse as MessageBatchMarkMessageReadResponse, BatchSendMessageRequest,
    BatchSendMessageResponse, DeleteMessageRequest as MessageDeleteMessageRequest,
    DeleteMessageResponse as MessageDeleteMessageResponse,
    EditMessageRequest as MessageEditMessageRequest,
    EditMessageResponse as MessageEditMessageResponse,
    GetMessageRequest as MessageGetMessageRequest, GetMessageResponse as MessageGetMessageResponse,
    MarkAllConversationsReadRequest as MessageMarkAllConversationsReadRequest,
    MarkAllConversationsReadResponse as MessageMarkAllConversationsReadResponse,
    MarkConversationReadRequest as MessageMarkConversationReadRequest,
    MarkConversationReadResponse as MessageMarkConversationReadResponse,
    MarkMessageReadRequest as MessageMarkMessageReadRequest,
    MarkMessageReadResponse as MessageMarkMessageReadResponse,
    MarkMessageRequest as MessageMarkMessageRequest,
    MarkMessageResponse as MessageMarkMessageResponse,
    MarkMessagesReadUntilRequest as MessageMarkMessagesReadUntilRequest,
    MarkMessagesReadUntilResponse as MessageMarkMessagesReadUntilResponse,
    PinMessageRequest as MessagePinMessageRequest, PinMessageResponse as MessagePinMessageResponse,
    QueryMessagesRequest as MessageQueryMessagesRequest,
    QueryMessagesResponse as MessageQueryMessagesResponse,
    RecallMessageRequest as MessageRecallMessageRequest,
    RecallMessageResponse as MessageRecallMessageResponse,
    RemoveReactionRequest as MessageRemoveReactionRequest,
    RemoveReactionResponse as MessageRemoveReactionResponse,
    SearchMessagesRequest as MessageSearchMessagesRequest,
    SearchMessagesResponse as MessageSearchMessagesResponse, SendMessageRequest,
    SendMessageResponse, SendSystemMessageRequest, SendSystemMessageResponse,
    UnmarkMessageRequest as MessageUnmarkMessageRequest,
    UnmarkMessageResponse as MessageUnmarkMessageResponse,
    UnpinMessageRequest as MessageUnpinMessageRequest,
    UnpinMessageResponse as MessageUnpinMessageResponse,
};
use flare_proto::storage::StoreMessageRequest;
use prost_types;
use tonic::{Request, Response, Status};
use tracing::{error, info, instrument, warn};

use crate::application::commands::StoreMessageCommand;
use crate::application::handlers::{MessageCommandHandler, MessageQueryHandler};
use crate::application::utils::OperationMessageBuilder;
use crate::application::queries::QueryMessageQuery;
use flare_proto::message::message_service_server::MessageService;
use flare_im_core::utils::context::require_context;
use flare_server_core::context::Context;
use chrono::Utc;

/// 消息 gRPC 处理器 - 处理所有消息相关的 gRPC 请求（接口层）
///
/// 职责：
/// 1. 将 gRPC 请求转换为应用层命令/查询
/// 2. 调用应用层 handlers
/// 3. 构建 gRPC 响应
///
/// 架构原则：
/// - 接口层不包含业务逻辑
/// - 所有业务处理都委托给应用层（CommandHandler/QueryHandler）
/// - 只负责协议转换和错误处理
#[derive(Clone)]
pub struct MessageGrpcHandler {
    command_handler: Arc<MessageCommandHandler>,
    query_handler: Arc<MessageQueryHandler>,
}

impl MessageGrpcHandler {
    pub fn new(
        command_handler: Arc<MessageCommandHandler>,
        query_handler: Arc<MessageQueryHandler>,
    ) -> Self {
        Self {
            command_handler,
            query_handler,
        }
    }
}

    #[tonic::async_trait]
    impl MessageService for MessageGrpcHandler {
    #[instrument(skip(self, request))]
        async fn send_message(
        &self,
        request: Request<SendMessageRequest>,
    ) -> Result<Response<SendMessageResponse>, Status> {
            // 从请求中提取 Context
            let ctx = require_context(&request)?;
            
        let req = request.into_inner();
        let message = req
            .message
                .clone()
            .ok_or_else(|| Status::invalid_argument("message required"))?;

            // 构建发送消息命令
            let cmd = crate::application::commands::SendMessageCommand {
                message,
            conversation_id: req.conversation_id.clone(),
            sync: req.sync,
            context: req.context.clone(),
                tenant: req.tenant.clone(),
        };

            // 调用应用层处理器处理发送消息逻辑
            match self.command_handler.handle_send_message(&ctx, cmd).await {
            Ok((message_id, seq)) => {
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
                        seq,
                    sent_at: Some(prost_types::Timestamp {
                        seconds: now.timestamp(),
                        nanos: now.timestamp_subsec_nanos() as i32,
                    }),
                    timeline,
                    status: Some(ok_status()),
                }))
            }
            Err(err) => {
                    error!(error = %err, "Failed to send message");
                Err(Status::internal(err.to_string()))
            }
        }
    }

    #[instrument(skip(self, request))]
        async fn batch_send_message(
        &self,
        request: Request<BatchSendMessageRequest>,
    ) -> Result<Response<BatchSendMessageResponse>, Status> {
            // 从请求中提取 Context
            let ctx = require_context(&request)?;

        let req = request.into_inner();

            // 构建批量发送消息命令
            let cmd = crate::application::commands::BatchSendMessageCommand {
                requests: req.messages,
            };

            // 调用应用层处理器处理批量发送消息逻辑
            match self.command_handler.handle_batch_send_message(&ctx, cmd).await {
                Ok((successes, failure_messages)) => {
                    let success_count = successes.len() as i32;
                    let fail_count = failure_messages.len() as i32;
        let mut message_ids = Vec::new();
        let mut failures = Vec::new();

                    for (message_id, _seq) in successes {
                    message_ids.push(message_id);
                }

                    for error_msg in failure_messages {
                    failures.push(flare_proto::message::FailedMessage {
                        message_id: String::new(),
                        code: 500, // InternalError
                            error_message: error_msg,
                    });
        }

        Ok(Response::new(BatchSendMessageResponse {
            success_count,
            fail_count,
            message_ids,
            failures,
            status: Some(ok_status()),
        }))
    }
                Err(err) => {
                    error!(error = %err, "Failed to batch send messages");
                    Err(Status::internal(err.to_string()))
                }
            }
        }
    #[instrument(skip(self, request))]
        async fn send_system_message(
        &self,
        request: Request<SendSystemMessageRequest>,
    ) -> Result<Response<SendSystemMessageResponse>, Status> {
            // 从请求中提取 Context
            let ctx = require_context(&request)?;
            
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

    #[instrument(skip(self, request))]
        async fn recall_message(
        &self,
            request: Request<MessageRecallMessageRequest>,
        ) -> Result<Response<MessageRecallMessageResponse>, Status> {
        let req = request.into_inner();

            // 从请求上下文提取操作者ID
            let operator_id = req
                .context
                .as_ref()
                .and_then(|c| c.actor.as_ref())
                .map(|a| a.actor_id.clone())
                .unwrap_or_default();

            // 查询原消息获取 conversation_id
            let original_message = self
                .query_handler
                .query_message(QueryMessageQuery {
                    message_id: req.message_id.clone(),
                    conversation_id: String::new(),
                })
                .await
                .map_err(|e| {
                    if e.to_string().contains("not found") {
                        Status::not_found(format!("Message not found: {}", req.message_id))
                    } else {
                        Status::internal(format!("Failed to query message: {}", e))
                    }
                })?;

            let conversation_id = original_message.conversation_id.clone();

            // 构建操作消息
            let operation_message = OperationMessageBuilder::build_recall_message(
                &req,
                &conversation_id,
                &operator_id,
            )
            .map_err(|e| Status::internal(format!("Failed to build recall message: {}", e)))?;

            // 构建 SendMessageRequest
            let send_req = SendMessageRequest {
                conversation_id: conversation_id.clone(),
                message: Some(operation_message),
                sync: false, // 操作消息默认异步
                context: req.context.clone(),
                tenant: req.tenant.clone(),
            };

            // 调用 SendMessage（统一处理）
            let send_resp = self.send_message(Request::new(send_req)).await?;
            let send_inner = send_resp.into_inner();

            // 转换为 RecallMessageResponse
            Ok(Response::new(MessageRecallMessageResponse {
                success: send_inner.success,
                error_message: if send_inner.success {
                    String::new()
                } else {
                    send_inner.status.as_ref()
                        .map(|s| s.message.clone())
                        .unwrap_or_default()
                },
                recalled_at: send_inner.sent_at,
                status: send_inner.status,
            }))
        }

    #[instrument(skip(self, request))]
        async fn edit_message(
        &self,
        request: Request<MessageEditMessageRequest>,
    ) -> Result<Response<MessageEditMessageResponse>, Status> {
        let req = request.into_inner();

        // 从请求上下文提取操作者ID
        let operator_id = req
            .context
            .as_ref()
            .and_then(|c| c.actor.as_ref())
            .map(|a| a.actor_id.clone())
            .unwrap_or_default();

        // 查询原消息获取 conversation_id
        let original_message = self
            .query_handler
            .query_message(QueryMessageQuery {
                message_id: req.message_id.clone(),
                conversation_id: String::new(),
            })
            .await
            .map_err(|e| {
                if e.to_string().contains("not found") {
                    Status::not_found(format!("Message not found: {}", req.message_id))
                } else {
                    Status::internal(format!("Failed to query message: {}", e))
                }
            })?;

        let conversation_id = original_message.conversation_id.clone();

        // 构建操作消息
        let operation_message = OperationMessageBuilder::build_edit_message(
            &req,
            &conversation_id,
            &operator_id,
        )
        .map_err(|e| Status::internal(format!("Failed to build edit message: {}", e)))?;

        // 构建 SendMessageRequest
        let send_req = SendMessageRequest {
            conversation_id: conversation_id.clone(),
            message: Some(operation_message),
            sync: false, // 操作消息默认异步
            context: req.context.clone(),
            tenant: req.tenant.clone(),
        };

        // 调用 SendMessage（统一处理）
        let send_resp = self.send_message(Request::new(send_req)).await?;
        let send_inner = send_resp.into_inner();

        // 转换为 EditMessageResponse
        Ok(Response::new(MessageEditMessageResponse {
            success: send_inner.success,
            error_message: if send_inner.success {
                String::new()
            } else {
                send_inner.status.as_ref()
                    .map(|s| s.message.clone())
                    .unwrap_or_default()
            },
            message_id: req.message_id,
            edit_version: req.edit_version,
            edited_at: send_inner.sent_at,
            status: send_inner.status,
        }))
    }

    #[instrument(skip(self, request))]
        async fn delete_message(
        &self,
            request: Request<MessageDeleteMessageRequest>,
        ) -> Result<Response<MessageDeleteMessageResponse>, Status> {
        let req = request.into_inner();

        // 从请求上下文提取操作者ID
        let operator_id = req
            .context
            .as_ref()
            .and_then(|c| c.actor.as_ref())
            .map(|a| a.actor_id.clone())
            .unwrap_or_default();

        // 构建操作消息（delete_message 请求中已有 conversation_id）
        let operation_message = OperationMessageBuilder::build_delete_message(
            &req,
            &operator_id,
        )
        .map_err(|e| Status::internal(format!("Failed to build delete message: {}", e)))?;

        // 构建 SendMessageRequest
        let send_req = SendMessageRequest {
            conversation_id: req.conversation_id.clone(),
            message: Some(operation_message),
            sync: false, // 操作消息默认异步
            context: req.context.clone(),
            tenant: req.tenant.clone(),
        };

        // 调用 SendMessage（统一处理）
        let send_resp = self.send_message(Request::new(send_req)).await?;
        let send_inner = send_resp.into_inner();

        // 转换为 DeleteMessageResponse
        // 注意：批量删除时，实际删除数量需要从操作结果中获取
        // 这里简化处理，返回成功表示至少删除了一条消息
        Ok(Response::new(MessageDeleteMessageResponse {
            success: send_inner.success,
            deleted_count: if send_inner.success {
                req.message_ids.len() as i32
            } else {
                0
            },
            status: send_inner.status,
        }))
        }

    #[instrument(skip(self, request))]
        async fn mark_message_read(
        &self,
            request: Request<MessageMarkMessageReadRequest>,
        ) -> Result<Response<MessageMarkMessageReadResponse>, Status> {
        let req = request.into_inner();

        // 查询原消息获取 conversation_id
        let original_message = self
            .query_handler
            .query_message(QueryMessageQuery {
                message_id: req.message_id.clone(),
                conversation_id: String::new(),
            })
            .await
            .map_err(|e| {
                if e.to_string().contains("not found") {
                    Status::not_found(format!("Message not found: {}", req.message_id))
                } else {
                    Status::internal(format!("Failed to query message: {}", e))
                }
            })?;

        let conversation_id = original_message.conversation_id.clone();

        // 构建已读操作消息（使用 MessageOperation）
        let now = Utc::now();
        let timestamp = prost_types::Timestamp {
            seconds: now.timestamp(),
            nanos: now.timestamp_subsec_nanos() as i32,
        };

        let operation = flare_proto::common::MessageOperation {
            operation_type: flare_proto::common::OperationType::Read as i32,
            target_message_id: req.message_id.clone(),
            operator_id: req.user_id.clone(),
            timestamp: Some(timestamp.clone()),
            show_notice: false, // 已读操作不显示通知
            notice_text: String::new(),
            target_user_id: String::new(),
            operation_data: Some(flare_proto::common::message_operation::OperationData::Read(
                flare_proto::common::ReadOperationData {
                    message_ids: vec![req.message_id.clone()],
                    read_at: req.read_at.clone().or(Some(timestamp)),
                    burn_after_read: req.burn_after_read,
                },
            )),
            metadata: std::collections::HashMap::new(),
        };

        let mut operation_message = flare_proto::common::Message::default();
        operation_message.server_id = format!("op_{}", uuid::Uuid::new_v4());
        operation_message.conversation_id = conversation_id.clone();
        operation_message.sender_id = req.user_id.clone();
        operation_message.message_type = flare_proto::MessageType::Operation as i32;
        operation_message.timestamp = Some(timestamp.clone());
        operation_message.content = Some(flare_proto::common::MessageContent {
            content: Some(flare_proto::common::message_content::Content::Operation(operation)),
            extensions: vec![],
        });
        operation_message.extra.insert("message_type".to_string(), "operation".to_string());
        operation_message.extra.insert("operation_type".to_string(), "read".to_string());

        // 构建 SendMessageRequest
        let send_req = SendMessageRequest {
            conversation_id: conversation_id.clone(),
            message: Some(operation_message),
            sync: false, // 已读操作默认异步
            context: req.context.clone(),
            tenant: req.tenant.clone(),
        };

        // 调用 SendMessage（统一处理）
        let send_resp = self.send_message(Request::new(send_req)).await?;
        let send_inner = send_resp.into_inner();

        // 转换为 MarkMessageReadResponse
        Ok(Response::new(MessageMarkMessageReadResponse {
            success: send_inner.success,
            error_message: if send_inner.success {
                String::new()
            } else {
                send_inner.status.as_ref()
                    .map(|s| s.message.clone())
                    .unwrap_or_default()
            },
            read_at: send_inner.sent_at,
            burned_at: None, // 阅后即焚时间需要从操作结果中获取
            status: send_inner.status,
        }))
        }

    #[instrument(skip(self, request))]
        async fn batch_mark_message_read(
        &self,
        request: Request<MessageBatchMarkMessageReadRequest>,
    ) -> Result<Response<MessageBatchMarkMessageReadResponse>, Status> {
        let ctx = require_context(&request)?;
        let req = request.into_inner();

            let operator_id = req.user_id.clone();
            let tenant_id = ctx.tenant_id().unwrap_or("0").to_string();

        let read_at = req
            .read_at
            .map(|ts| {
                chrono::DateTime::<chrono::Utc>::from_timestamp(ts.seconds, ts.nanos as u32)
                    .unwrap_or_else(|| chrono::Utc::now())
            })
            .unwrap_or_else(|| chrono::Utc::now());

            // 批量标记指定的消息为已读
            let mut read_count = 0i32;
            for message_id in &req.message_ids {
                let read_cmd = crate::application::commands::ReadMessageCommand {
                    base: crate::application::commands::MessageOperationCommand {
                    message_id: message_id.clone(),
                        operator_id: operator_id.clone(),
                        timestamp: chrono::Utc::now(),
                        tenant_id: tenant_id.clone(),
                        conversation_id: req.conversation_id.clone(),
                    },
                    message_ids: vec![message_id.clone()],
                    read_at: Some(read_at),
                    burn_after_read: false,
                };

                if self.command_handler.handle_read_message(read_cmd).await.is_ok() {
                            read_count += 1;
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

        async fn mark_messages_read_until(
            &self,
            _request: Request<MessageMarkMessagesReadUntilRequest>,
        ) -> Result<Response<MessageMarkMessagesReadUntilResponse>, Status> {
            Err(Status::unimplemented("mark_messages_read_until not implemented"))
        }

    #[instrument(skip(self, request))]
        async fn mark_conversation_read(
        &self,
        request: Request<MessageMarkConversationReadRequest>,
    ) -> Result<Response<MessageMarkConversationReadResponse>, Status> {
        let ctx = require_context(&request)?;
        let req = request.into_inner();

            let tenant_id = ctx.tenant_id().unwrap_or("0").to_string();

            let read_at = req
                .read_at
                .map(|ts| {
                    chrono::DateTime::<chrono::Utc>::from_timestamp(ts.seconds, ts.nanos as u32)
                        .unwrap_or_else(|| chrono::Utc::now())
                })
                .unwrap_or_else(|| chrono::Utc::now());

            // 构建标记会话已读命令
            let cmd = crate::application::commands::MarkConversationReadCommand {
                conversation_id: req.conversation_id.clone(),
                user_id: req.user_id.clone(),
                read_at: Some(read_at),
                tenant_id,
            };

            // 调用应用层处理器处理标记会话已读逻辑
            // 注意：MarkConversationReadCommand 需要在 command_handler 中实现
            // 这里先通过批量标记已读来实现
        let batch_req = MessageBatchMarkMessageReadRequest {
            conversation_id: req.conversation_id.clone(),
            user_id: req.user_id.clone(),
            message_ids: vec![], // 空列表表示标记会话中所有未读消息
            read_at: req.read_at.clone(),
            burn_after_read: req.burn_after_read,
            context: req.context.clone(),
            tenant: req.tenant.clone(),
        };

        let batch_resp = self.batch_mark_message_read(Request::new(batch_req)).await?.into_inner();

        // 获取会话的最后一条消息ID作为 last_read_message_id
            let query = crate::application::queries::QueryMessagesQuery {
            conversation_id: req.conversation_id.clone(),
                limit: Some(1),
                cursor: None,
                start_time: None,
                end_time: None,
        };

        let last_read_message_id = self
                .query_handler
                .query_messages(query)
            .await
            .ok()
                .and_then(|messages| messages.first().map(|m| m.server_id.clone()))
            .unwrap_or_default();

        Ok(Response::new(MessageMarkConversationReadResponse {
            success: batch_resp.success,
            error_message: batch_resp.error_message,
            read_count: batch_resp.read_count,
            read_at: Some(prost_types::Timestamp {
                seconds: read_at.timestamp(),
                nanos: read_at.timestamp_subsec_nanos() as i32,
            }),
            last_read_message_id: if !last_read_message_id.is_empty() {
                last_read_message_id
            } else {
                String::new()
            },
            status: batch_resp.status,
        }))
    }

    #[instrument(skip(self, request))]
        async fn mark_all_conversations_read(
        &self,
        request: Request<MessageMarkAllConversationsReadRequest>,
    ) -> Result<Response<MessageMarkAllConversationsReadResponse>, Status> {
        let _req = request.into_inner();

        // 这里需要查询用户的所有会话，然后对每个会话调用 mark_conversation_read
        // 简化实现：返回未实现错误，实际应该查询用户会话列表并批量处理
        Err(Status::unimplemented("mark_all_conversations_read requires conversation service integration"))
    }

    #[instrument(skip(self, request))]
        async fn add_reaction(
        &self,
            request: Request<MessageAddReactionRequest>,
        ) -> Result<Response<MessageAddReactionResponse>, Status> {
        let req = request.into_inner();

        // 查询原消息获取 conversation_id
        let original_message = self
            .query_handler
            .query_message(QueryMessageQuery {
                message_id: req.message_id.clone(),
                conversation_id: String::new(),
            })
            .await
            .map_err(|e| {
                if e.to_string().contains("not found") {
                    Status::not_found(format!("Message not found: {}", req.message_id))
                } else {
                    Status::internal(format!("Failed to query message: {}", e))
                }
            })?;

        let conversation_id = original_message.conversation_id.clone();

        // 构建操作消息
        let operation_message = OperationMessageBuilder::build_add_reaction_message(
            &req,
            &conversation_id,
        )
        .map_err(|e| Status::internal(format!("Failed to build add reaction message: {}", e)))?;

        // 构建 SendMessageRequest
        let send_req = SendMessageRequest {
            conversation_id: conversation_id.clone(),
            message: Some(operation_message),
            sync: false, // 操作消息默认异步
            context: req.context.clone(),
            tenant: req.tenant.clone(),
        };

        // 调用 SendMessage（统一处理）
        let send_resp = self.send_message(Request::new(send_req)).await?;
        let send_inner = send_resp.into_inner();

        // 转换为 AddReactionResponse
        // 注意：new_count 需要从操作结果中获取，这里简化处理
        Ok(Response::new(MessageAddReactionResponse {
            success: send_inner.success,
            error_message: if send_inner.success {
                String::new()
            } else {
                send_inner.status.as_ref()
                    .map(|s| s.message.clone())
                    .unwrap_or_default()
            },
            new_count: 0, // 需要从操作结果中获取
            status: send_inner.status,
        }))
    }

    #[instrument(skip(self, request))]
        async fn remove_reaction(
        &self,
            request: Request<MessageRemoveReactionRequest>,
        ) -> Result<Response<MessageRemoveReactionResponse>, Status> {
            let req = request.into_inner();

            // 查询原消息获取 conversation_id
            let original_message = self
                .query_handler
                .query_message(QueryMessageQuery {
                    message_id: req.message_id.clone(),
                    conversation_id: String::new(),
                })
                .await
                .map_err(|e| {
                    if e.to_string().contains("not found") {
                        Status::not_found(format!("Message not found: {}", req.message_id))
                    } else {
                        Status::internal(format!("Failed to query message: {}", e))
                    }
                })?;

            let conversation_id = original_message.conversation_id.clone();

            // 构建操作消息
            let operation_message = OperationMessageBuilder::build_remove_reaction_message(
                &req,
                &conversation_id,
            )
            .map_err(|e| Status::internal(format!("Failed to build remove reaction message: {}", e)))?;

            // 构建 SendMessageRequest
            let send_req = SendMessageRequest {
                conversation_id: conversation_id.clone(),
                message: Some(operation_message),
                sync: false, // 操作消息默认异步
                context: req.context.clone(),
                tenant: req.tenant.clone(),
            };

            // 调用 SendMessage（统一处理）
            let send_resp = self.send_message(Request::new(send_req)).await?;
            let send_inner = send_resp.into_inner();

            // 转换为 RemoveReactionResponse
            Ok(Response::new(MessageRemoveReactionResponse {
                success: send_inner.success,
                error_message: if send_inner.success {
                    String::new()
                } else {
                    send_inner.status.as_ref()
                        .map(|s| s.message.clone())
                        .unwrap_or_default()
                },
                new_count: 0, // 需要从操作结果中获取
                status: send_inner.status,
            }))
    }

    // reply_message 和 quote_message 已废弃：现在通过 SendMessage + Message.quote 字段实现

        #[instrument(skip(self, request))]
    async fn pin_message(
        &self,
        request: Request<MessagePinMessageRequest>,
    ) -> Result<Response<MessagePinMessageResponse>, Status> {
            let req = request.into_inner();

            // 查询原消息获取 conversation_id
            let original_message = self
                .query_handler
                .query_message(QueryMessageQuery {
                    message_id: req.message_id.clone(),
                    conversation_id: String::new(),
                })
                .await
                .map_err(|e| {
                    if e.to_string().contains("not found") {
                        Status::not_found(format!("Message not found: {}", req.message_id))
                    } else {
                        Status::internal(format!("Failed to query message: {}", e))
                    }
                })?;

            let conversation_id = original_message.conversation_id.clone();

            // 构建操作消息
            let operation_message = OperationMessageBuilder::build_pin_message(
                &req,
                &conversation_id,
            )
            .map_err(|e| Status::internal(format!("Failed to build pin message: {}", e)))?;

            // 构建 SendMessageRequest
            let send_req = SendMessageRequest {
                conversation_id: conversation_id.clone(),
                message: Some(operation_message),
                sync: false, // 操作消息默认异步
                context: req.context.clone(),
                tenant: req.tenant.clone(),
            };

            // 调用 SendMessage（统一处理）
            let send_resp = self.send_message(Request::new(send_req)).await?;
            let send_inner = send_resp.into_inner();

            // 转换为 PinMessageResponse
            Ok(Response::new(MessagePinMessageResponse {
                success: send_inner.success,
                error_message: if send_inner.success {
                    String::new()
                } else {
                    send_inner.status.as_ref()
                        .map(|s| s.message.clone())
                        .unwrap_or_default()
                },
                pinned_at: send_inner.sent_at,
                status: send_inner.status,
            }))
        }

        #[instrument(skip(self, request))]
    async fn unpin_message(
        &self,
        request: Request<MessageUnpinMessageRequest>,
    ) -> Result<Response<MessageUnpinMessageResponse>, Status> {
            let req = request.into_inner();

            // 查询原消息获取 conversation_id
            let original_message = self
                .query_handler
                .query_message(QueryMessageQuery {
                    message_id: req.message_id.clone(),
                    conversation_id: String::new(),
                })
                .await
                .map_err(|e| {
                    if e.to_string().contains("not found") {
                        Status::not_found(format!("Message not found: {}", req.message_id))
                    } else {
                        Status::internal(format!("Failed to query message: {}", e))
                    }
                })?;

            let conversation_id = original_message.conversation_id.clone();

            // 构建操作消息
            let operation_message = OperationMessageBuilder::build_unpin_message(
                &req,
                &conversation_id,
            )
            .map_err(|e| Status::internal(format!("Failed to build unpin message: {}", e)))?;

            // 构建 SendMessageRequest
            let send_req = SendMessageRequest {
                conversation_id: conversation_id.clone(),
                message: Some(operation_message),
                sync: false, // 操作消息默认异步
                context: req.context.clone(),
                tenant: req.tenant.clone(),
            };

            // 调用 SendMessage（统一处理）
            let send_resp = self.send_message(Request::new(send_req)).await?;
            let send_inner = send_resp.into_inner();

            // 转换为 UnpinMessageResponse
            Ok(Response::new(MessageUnpinMessageResponse {
                success: send_inner.success,
                error_message: if send_inner.success {
                    String::new()
                } else {
                    send_inner.status.as_ref()
                        .map(|s| s.message.clone())
                        .unwrap_or_default()
                },
                status: send_inner.status,
            }))
        }

        #[instrument(skip(self, request))]
    async fn mark_message(
        &self,
        request: Request<MessageMarkMessageRequest>,
    ) -> Result<Response<MessageMarkMessageResponse>, Status> {
            let req = request.into_inner();

            // 查询原消息获取 conversation_id
            let original_message = self
                .query_handler
                .query_message(QueryMessageQuery {
                    message_id: req.message_id.clone(),
                    conversation_id: String::new(),
                })
                .await
                .map_err(|e| {
                    if e.to_string().contains("not found") {
                        Status::not_found(format!("Message not found: {}", req.message_id))
                    } else {
                        Status::internal(format!("Failed to query message: {}", e))
                    }
                })?;

            let conversation_id = original_message.conversation_id.clone();

            // 构建操作消息
            let operation_message = OperationMessageBuilder::build_mark_message(
                &req,
                &conversation_id,
            )
            .map_err(|e| Status::internal(format!("Failed to build mark message: {}", e)))?;

            // 构建 SendMessageRequest
            let send_req = SendMessageRequest {
                conversation_id: conversation_id.clone(),
                message: Some(operation_message),
                sync: false, // 操作消息默认异步
                context: req.context.clone(),
                tenant: req.tenant.clone(),
            };

            // 调用 SendMessage（统一处理）
            let send_resp = self.send_message(Request::new(send_req)).await?;
            let send_inner = send_resp.into_inner();

            // 转换为 MarkMessageResponse
            Ok(Response::new(MessageMarkMessageResponse {
                success: send_inner.success,
                error_message: if send_inner.success {
                    String::new()
                } else {
                    send_inner.status.as_ref()
                        .map(|s| s.message.clone())
                        .unwrap_or_default()
                },
                marked_at: send_inner.sent_at,
                status: send_inner.status,
            }))
        }

        #[instrument(skip(self, request))]
    async fn unmark_message(
        &self,
        request: Request<MessageUnmarkMessageRequest>,
    ) -> Result<Response<MessageUnmarkMessageResponse>, Status> {
            let req = request.into_inner();

            // 查询原消息获取 conversation_id
            let original_message = self
                .query_handler
                .query_message(QueryMessageQuery {
                    message_id: req.message_id.clone(),
                    conversation_id: String::new(),
                })
                .await
                .map_err(|e| {
                    if e.to_string().contains("not found") {
                        Status::not_found(format!("Message not found: {}", req.message_id))
                    } else {
                        Status::internal(format!("Failed to query message: {}", e))
                    }
                })?;

            let conversation_id = original_message.conversation_id.clone();

            // 构建操作消息
            let operation_message = OperationMessageBuilder::build_unmark_message(
                &req,
                &conversation_id,
            )
            .map_err(|e| Status::internal(format!("Failed to build unmark message: {}", e)))?;

            // 构建 SendMessageRequest
            let send_req = SendMessageRequest {
                conversation_id: conversation_id.clone(),
                message: Some(operation_message),
                sync: false, // 操作消息默认异步
                context: req.context.clone(),
                tenant: req.tenant.clone(),
            };

            // 调用 SendMessage（统一处理）
            let send_resp = self.send_message(Request::new(send_req)).await?;
            let send_inner = send_resp.into_inner();

            // 转换为 UnmarkMessageResponse
            Ok(Response::new(MessageUnmarkMessageResponse {
                success: send_inner.success,
                error_message: if send_inner.success {
                    String::new()
                } else {
                    send_inner.status.as_ref()
                        .map(|s| s.message.clone())
                        .unwrap_or_default()
                },
                status: send_inner.status,
            }))
        }

        #[instrument(skip(self, request))]
    async fn query_messages(
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

        #[instrument(skip(self, request))]
    async fn search_messages(
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

        #[instrument(skip(self, request))]
    async fn get_message(
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

    async fn get_pinned_messages(
        &self,
        _request: Request<flare_proto::message::GetPinnedMessagesRequest>,
    ) -> Result<Response<flare_proto::message::GetPinnedMessagesResponse>, Status> {
        Err(Status::unimplemented("get_pinned_messages not implemented"))
    }

    async fn get_marked_messages(
        &self,
        _request: Request<flare_proto::message::GetMarkedMessagesRequest>,
    ) -> Result<Response<flare_proto::message::GetMarkedMessagesResponse>, Status> {
        Err(Status::unimplemented("get_marked_messages not implemented"))
    }

    async fn get_threads(
        &self,
        _request: Request<flare_proto::message::GetThreadsRequest>,
    ) -> Result<Response<flare_proto::message::GetThreadsResponse>, Status> {
        Err(Status::unimplemented("get_threads not implemented"))
    }

    async fn get_thread_replies(
        &self,
        _request: Request<flare_proto::message::GetThreadRepliesRequest>,
    ) -> Result<Response<flare_proto::message::GetThreadRepliesResponse>, Status> {
        Err(Status::unimplemented("get_thread_replies not implemented"))
    }
}


