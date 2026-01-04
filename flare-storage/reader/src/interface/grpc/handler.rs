use std::sync::Arc;

use chrono::{TimeZone, Utc};
use flare_proto::common::OperationType;
use flare_proto::storage::storage_reader_service_server::StorageReaderService;
use flare_proto::storage::*;
use tonic::{Request, Response, Status};
use tracing::error;

use crate::application::commands::{
    ClearConversationCommand, DeleteMessageCommand, DeleteMessageForUserCommand, ExportMessagesCommand,
    MarkReadCommand, RecallMessageCommand, SetMessageAttributesCommand,
};
use crate::application::handlers::{MessageStorageCommandHandler, MessageStorageQueryHandler};
use crate::application::queries::{
    GetMessageQuery, ListMessageTagsQuery, QueryMessagesBySeqQuery, QueryMessagesQuery,
    SearchMessagesQuery,
};

#[derive(Clone)]
pub struct StorageReaderGrpcHandler {
    command_handler: Arc<MessageStorageCommandHandler>,
    query_handler: Arc<MessageStorageQueryHandler>,
}

impl StorageReaderGrpcHandler {
    pub async fn new(
        command_handler: Arc<MessageStorageCommandHandler>,
        query_handler: Arc<MessageStorageQueryHandler>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            command_handler,
            query_handler,
        })
    }
}

#[tonic::async_trait]
impl StorageReaderService for StorageReaderGrpcHandler {
    async fn query_messages(
        &self,
        request: Request<QueryMessagesRequest>,
    ) -> Result<Response<QueryMessagesResponse>, Status> {
        let req = request.into_inner();
        let cursor_clone = req.cursor.clone();
        let query = QueryMessagesQuery {
            conversation_id: req.conversation_id,
            start_time: req.start_time,
            end_time: req.end_time,
            limit: req.limit,
            cursor: if req.cursor.is_empty() {
                None
            } else {
                Some(req.cursor)
            },
        };

        match self
            .query_handler
            .handle_query_messages_with_pagination(query)
            .await
        {
            Ok(result) => {
                Ok(Response::new(QueryMessagesResponse {
                    messages: result.messages,
                    next_cursor: result.next_cursor.clone(),
                    has_more: result.has_more,
                    pagination: Some(flare_proto::common::Pagination {
                        cursor: cursor_clone,
                        limit: req.limit,
                        has_more: result.has_more,
                        previous_cursor: String::new(),
                        total_size: result.total_size,
                    }),
                    status: Some(flare_server_core::error::ok_status()),
                }))
            }
            Err(err) => {
                error!(error = ?err, "Failed to query messages");
                Err(Status::internal(err.to_string()))
            }
        }
    }

    async fn query_messages_by_seq(
        &self,
        request: Request<flare_proto::storage::QueryMessagesBySeqRequest>,
    ) -> Result<Response<flare_proto::storage::QueryMessagesBySeqResponse>, Status> {
        let req = request.into_inner();
        let query = QueryMessagesBySeqQuery {
            conversation_id: req.conversation_id,
            after_seq: req.after_seq,
            before_seq: if req.before_seq == 0 {
                None
            } else {
                Some(req.before_seq)
            },
            limit: req.limit,
            user_id: if req.user_id.is_empty() {
                None
            } else {
                Some(req.user_id)
            },
        };

        match self.query_handler.handle_query_messages_by_seq(query).await {
            Ok((messages, last_seq)) => {
                let message_count = messages.len() as i32;
                // 构建基于 seq 的游标
                let next_cursor = messages
                    .last()
                    .and_then(|msg| {
                        msg.extra
                            .get("seq")
                            .map(|seq_str| format!("seq:{}:{}", seq_str, msg.server_id))
                    })
                    .unwrap_or_default();
                let has_more = message_count >= req.limit;

                Ok(Response::new(
                    flare_proto::storage::QueryMessagesBySeqResponse {
                        messages,
                        next_cursor: next_cursor.clone(),
                        has_more,
                        last_seq: last_seq.unwrap_or(0),
                        status: Some(flare_server_core::error::ok_status()),
                    },
                ))
            }
            Err(err) => {
                error!(error = ?err, "Failed to query messages by seq");
                Err(Status::internal(err.to_string()))
            }
        }
    }

    async fn get_message(
        &self,
        request: Request<GetMessageRequest>,
    ) -> Result<Response<GetMessageResponse>, Status> {
        let req = request.into_inner();
        let query = GetMessageQuery {
            message_id: req.message_id,
        };

        match self.query_handler.handle_get_message(query).await {
            Ok(message) => Ok(Response::new(GetMessageResponse {
                message,
                status: Some(flare_server_core::error::ok_status()),
            })),
            Err(err) => {
                error!(error = ?err, "Failed to get message");
                Err(Status::internal(err.to_string()))
            }
        }
    }

    async fn delete_message(
        &self,
        request: Request<DeleteMessageRequest>,
    ) -> Result<Response<DeleteMessageResponse>, Status> {
        let req = request.into_inner();
        let command = DeleteMessageCommand {
            message_ids: req.message_ids,
        };

        match self.command_handler.handle_delete_message(command).await {
            Ok(deleted_count) => Ok(Response::new(DeleteMessageResponse {
                success: deleted_count > 0,
                deleted_count: deleted_count as i32,
                status: Some(flare_server_core::error::ok_status()),
            })),
            Err(err) => {
                error!(error = ?err, "Failed to delete message");
                Err(Status::internal(err.to_string()))
            }
        }
    }

    async fn recall_message(
        &self,
        request: Request<RecallMessageRequest>,
    ) -> Result<Response<RecallMessageResponse>, Status> {
        let req = request.into_inner();
        let command = RecallMessageCommand {
            message_id: req.message_id,
            recall_time_limit_seconds: req.recall_time_limit_seconds as i64,
        };

        match self.command_handler.handle_recall_message(command).await {
            Ok(recalled_at) => Ok(Response::new(RecallMessageResponse {
                success: true,
                error_message: String::new(),
                recalled_at,
                status: Some(flare_server_core::error::ok_status()),
            })),
            Err(err) => {
                error!(error = ?err, "Failed to recall message");
                Ok(Response::new(RecallMessageResponse {
                    success: false,
                    error_message: err.to_string(),
                    recalled_at: None,
                    status: Some(flare_server_core::error::ok_status()),
                }))
            }
        }
    }

    async fn delete_message_for_user(
        &self,
        request: Request<DeleteMessageForUserRequest>,
    ) -> Result<Response<DeleteMessageForUserResponse>, Status> {
        let ctx = flare_im_core::utils::context::require_context(&request)?;
        let req = request.into_inner();
        let command = DeleteMessageForUserCommand {
            message_ids: vec![req.message_id],
            user_id: req.user_id,
            conversation_id: String::new(),
            permanent: req.permanent,
        };

        match self
            .command_handler
            .handle_delete_message_for_user(&ctx, command)
            .await
        {
            Ok(deleted_count) => Ok(Response::new(DeleteMessageForUserResponse {
                success: deleted_count > 0,
                deleted_count: deleted_count as i32,
                status: Some(flare_server_core::error::ok_status()),
            })),
            Err(err) => {
                error!(error = ?err, "Failed to delete message for user");
                Err(Status::internal(err.to_string()))
            }
        }
    }

    async fn clear_conversation(
        &self,
        request: Request<ClearConversationRequest>,
    ) -> Result<Response<ClearConversationResponse>, Status> {
        let req = request.into_inner();

        // 解析 clear_before_time
        let clear_before_time = match req.clear_type() {
            flare_proto::storage::ClearType::ClearAll => None,
            flare_proto::storage::ClearType::ClearBeforeMessage => {
                // 需要先查询消息的时间戳
                if req.clear_before_message_id.is_empty() {
                    return Err(Status::invalid_argument(
                        "clear_before_message_id is required",
                    ));
                }
                // 查询消息时间戳
                match self
                    .query_handler
                    .handle_get_message_timestamp(&req.clear_before_message_id)
                    .await
                {
                    Ok(Some(timestamp)) => Some(timestamp),
                    Ok(None) => {
                        return Err(Status::not_found(format!(
                            "Message not found: {}",
                            req.clear_before_message_id
                        )));
                    }
                    Err(e) => {
                        error!(error = ?e, "Failed to get message timestamp");
                        return Err(Status::internal("Failed to get message timestamp"));
                    }
                }
            }
            flare_proto::storage::ClearType::ClearBeforeTime => req
                .clear_before_time
                .as_ref()
                .and_then(|ts| chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32)),
        };

        let command = ClearConversationCommand {
            conversation_id: req.conversation_id,
            user_id: Some(req.user_id),
            clear_before_time,
        };

        match self.command_handler.handle_clear_conversation(command).await {
            Ok(cleared_count) => {
                let cleared_at = Utc::now();
                Ok(Response::new(ClearConversationResponse {
                    success: true,
                    error_message: String::new(),
                    cleared_count: cleared_count as i32,
                    cleared_at: Some(prost_types::Timestamp {
                        seconds: cleared_at.timestamp(),
                        nanos: cleared_at.timestamp_subsec_nanos() as i32,
                    }),
                    status: Some(flare_server_core::error::ok_status()),
                }))
            }
            Err(err) => {
                error!(error = ?err, "Failed to clear session");
                Err(Status::internal(err.to_string()))
            }
        }
    }

    async fn mark_message_read(
        &self,
        request: Request<MarkMessageReadRequest>,
    ) -> Result<Response<MarkMessageReadResponse>, Status> {
        let ctx = flare_im_core::utils::context::require_context(&request)?;
        let req = request.into_inner();
        let command = MarkReadCommand {
            message_id: req.message_id,
            user_id: req.user_id,
        };

        match self.command_handler.handle_mark_read(&ctx, command).await {
            Ok((read_at, burned_at)) => Ok(Response::new(MarkMessageReadResponse {
                success: true,
                error_message: String::new(),
                read_at: Some(read_at),
                burned_at,
                status: Some(flare_server_core::error::ok_status()),
            })),
            Err(err) => {
                error!(error = ?err, "Failed to mark message as read");
                Ok(Response::new(MarkMessageReadResponse {
                    success: false,
                    error_message: err.to_string(),
                    read_at: None,
                    burned_at: None,
                    status: Some(flare_server_core::error::ok_status()),
                }))
            }
        }
    }

    async fn search_messages(
        &self,
        request: Request<SearchMessagesRequest>,
    ) -> Result<Response<SearchMessagesResponse>, Status> {
        let req = request.into_inner();

        // 解析时间范围
        let (start_time, end_time) = if let Some(time_range) = &req.time_range {
            let start = time_range
                .start_time
                .as_ref()
                .and_then(|ts| Utc.timestamp_opt(ts.seconds, ts.nanos as u32).single());
            let end = time_range
                .end_time
                .as_ref()
                .and_then(|ts| Utc.timestamp_opt(ts.seconds, ts.nanos as u32).single());
            (start, end)
        } else {
            (None, None)
        };

        let query = SearchMessagesQuery {
            filters: req.filters,
            start_time: start_time.map(|dt| dt.timestamp()).unwrap_or(0),
            end_time: end_time.map(|dt| dt.timestamp()).unwrap_or(0),
            limit: req.pagination.as_ref().map(|p| p.limit).unwrap_or(200),
        };

        match self.query_handler.handle_search_messages(query).await {
            Ok(messages) => {
                let pagination = req.pagination.clone().map(|mut p| {
                    p.has_more = messages.len() as i32 >= p.limit;
                    p
                });
                Ok(Response::new(SearchMessagesResponse {
                    messages,
                    pagination,
                    status: Some(flare_server_core::error::ok_status()),
                }))
            }
            Err(err) => {
                error!(error = ?err, "Failed to search messages");
                Err(Status::internal(err.to_string()))
            }
        }
    }

    async fn set_message_attributes(
        &self,
        request: Request<SetMessageAttributesRequest>,
    ) -> Result<Response<SetMessageAttributesResponse>, Status> {
        let req = request.into_inner();
        let attributes_map: std::collections::HashMap<String, String> =
            req.attributes.into_iter().collect();
        let command = SetMessageAttributesCommand {
            message_id: req.message_id.clone(),
            attributes: attributes_map.clone(),
            tags: req.tags.clone(),
        };

        // 从上下文提取操作者ID
        let operator_id = req
            .context
            .as_ref()
            .and_then(|ctx| ctx.actor.as_ref())
            .map(|actor| actor.actor_id.clone())
            .unwrap_or_default();

        // 判定操作类型
        let operation_type = if attributes_map.keys().any(|k| k.starts_with("edit")) {
            "edit"
        } else if attributes_map.keys().any(|k| k.starts_with("reaction:")) {
            "reaction"
        } else if attributes_map.keys().any(|k| k.starts_with("reply")) {
            "reply"
        } else if attributes_map.keys().any(|k| k.starts_with("thread")) {
            "thread"
        } else {
            "attributes_update"
        };

        // 构造操作记录
        let now = chrono::Utc::now();
        // 将操作类型字符串转换为 OperationType 枚举
        let operation_type_enum = match operation_type {
            "recall" => OperationType::Recall as i32,
            "edit" => OperationType::Edit as i32,
            "delete" => OperationType::Delete as i32,
            "read" => OperationType::Read as i32,
            // Reply 和 Quote 不在 OperationType 中，它们通过 Message.quote 字段处理
            "reaction_add" | "reaction" => OperationType::ReactionAdd as i32,
            "reaction_remove" => OperationType::ReactionRemove as i32,
            "pin" => OperationType::Pin as i32,
            "unpin" => OperationType::Unpin as i32,
            "mark" => OperationType::Mark as i32,
            _ => OperationType::Unspecified as i32,
        };

        let operation = flare_proto::common::MessageOperation {
            operation_type: operation_type_enum,
            target_message_id: req.message_id.clone(),
            operator_id,
            timestamp: Some(prost_types::Timestamp {
                seconds: now.timestamp(),
                nanos: now.timestamp_subsec_nanos() as i32,
            }),
            show_notice: false,
            notice_text: String::new(),
            target_user_id: String::new(),
            operation_data: None,
            metadata: {
                let mut meta = std::collections::HashMap::new();
                // 记录变更的属性键列表
                meta.insert(
                    "keys".to_string(),
                    attributes_map.keys().cloned().collect::<Vec<_>>().join(","),
                );
                meta
            },
        };

        match self
            .command_handler
            .handle_set_attributes_with_operation(command, operation)
            .await
        {
            Ok(()) => Ok(Response::new(SetMessageAttributesResponse {
                status: Some(flare_server_core::error::ok_status()),
            })),
            Err(err) => {
                error!(error = ?err, "Failed to set message attributes with operation");
                Err(Status::internal(err.to_string()))
            }
        }
    }

    async fn list_message_tags(
        &self,
        request: Request<ListMessageTagsRequest>,
    ) -> Result<Response<ListMessageTagsResponse>, Status> {
        let _req = request.into_inner();
        let query = ListMessageTagsQuery {};

        match self.query_handler.handle_list_message_tags(query).await {
            Ok(tags) => Ok(Response::new(ListMessageTagsResponse {
                tags,
                status: Some(flare_server_core::error::ok_status()),
            })),
            Err(err) => {
                error!(error = ?err, "Failed to list message tags");
                Ok(Response::new(ListMessageTagsResponse {
                    tags: vec![],
                    status: Some(flare_server_core::error::ok_status()),
                }))
            }
        }
    }

    async fn export_messages(
        &self,
        request: Request<ExportMessagesRequest>,
    ) -> Result<Response<ExportMessagesResponse>, Status> {
        let req = request.into_inner();
        let command = ExportMessagesCommand {
            conversation_id: req.conversation_id,
            start_time: req
                .time_range
                .as_ref()
                .and_then(|tr| tr.start_time.as_ref().map(|ts| ts.seconds)),
            end_time: req
                .time_range
                .as_ref()
                .and_then(|tr| tr.end_time.as_ref().map(|ts| ts.seconds)),
            limit: None, // ExportMessagesRequest 可能没有 limit 字段
        };

        match self.command_handler.handle_export_messages(command).await {
            Ok(export_task_id) => Ok(Response::new(ExportMessagesResponse {
                export_task_id,
                status: Some(flare_server_core::error::ok_status()),
            })),
            Err(err) => {
                error!(error = ?err, "Failed to export messages");
                Err(Status::internal(err.to_string()))
            }
        }
    }

    async fn add_or_remove_reaction(
        &self,
        request: Request<AddOrRemoveReactionRequest>,
    ) -> Result<Response<AddOrRemoveReactionResponse>, Status> {
        use flare_proto::storage::AddOrRemoveReactionResponse;

        let req = request.into_inner();

        // 通过 command_handler 处理反应操作
        let reactions = self.command_handler
            .handle_add_or_remove_reaction(
                &req.message_id,
                &req.emoji,
                &req.user_id,
                req.is_add,
            )
            .await
            .map_err(|e| {
                error!(error = ?e, message_id = %req.message_id, "Failed to add or remove reaction");
                Status::internal(format!("Failed to add or remove reaction: {}", e))
            })?;

        Ok(Response::new(AddOrRemoveReactionResponse {
            success: true,
            reactions,
            status: Some(flare_server_core::error::ok_status()),
        }))
    }
}
