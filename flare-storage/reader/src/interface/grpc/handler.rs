use std::sync::Arc;

use flare_proto::storage::*;
use flare_proto::storage::storage_reader_service_server::StorageReaderService;
use tonic::{Request, Response, Status};
use tracing::{error, info};
use chrono::{DateTime, TimeZone, Utc};

use crate::application::commands::{
    ClearSessionCommand, DeleteMessageCommand, DeleteMessageForUserCommand,
    ExportMessagesCommand, MarkReadCommand, RecallMessageCommand, SetMessageAttributesCommand,
};
use crate::application::queries::{
    GetMessageQuery, ListMessageTagsQuery, QueryMessagesQuery, QueryMessagesBySeqQuery, SearchMessagesQuery,
};
use crate::application::handlers::{
    MessageStorageCommandHandler, MessageStorageQueryHandler,
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
        let query = QueryMessagesQuery {
            session_id: req.session_id,
            start_time: req.start_time,
            end_time: req.end_time,
            limit: req.limit,
            cursor: if req.cursor.is_empty() {
                None
            } else {
                Some(req.cursor)
            },
        };

        let original_cursor = query.cursor.clone();
        match self.query_handler.handle_query_messages(query).await {
            Ok(messages) => {
                let message_count = messages.len() as i32;
                // 构建简单的游标（基于最后一条消息）
                let next_cursor = messages.last()
                    .and_then(|msg| msg.timestamp.as_ref())
                    .map(|ts| format!("{}:{}", ts.seconds, messages.last().unwrap().id.clone()))
                    .unwrap_or_default();
                let has_more = message_count >= req.limit;
                
                Ok(Response::new(QueryMessagesResponse {
                    messages,
                    next_cursor: next_cursor.clone(),
                    has_more,
                    pagination: Some(flare_proto::common::Pagination {
                        cursor: original_cursor.unwrap_or_default(),
                        limit: message_count,
                        has_more,
                        previous_cursor: String::new(),
                        total_size: message_count as i64, // 简化：使用当前返回数量
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
            session_id: req.session_id,
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
                            .map(|seq_str| format!("seq:{}:{}", seq_str, msg.id))
                    })
                    .unwrap_or_default();
                let has_more = message_count >= req.limit;

                Ok(Response::new(flare_proto::storage::QueryMessagesBySeqResponse {
                    messages,
                    next_cursor: next_cursor.clone(),
                    has_more,
                    last_seq: last_seq.unwrap_or(0),
                    status: Some(flare_server_core::error::ok_status()),
                }))
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
        let req = request.into_inner();
        // 注意：proto 中可能没有 session_id 字段，需要从消息中获取或使用空字符串
        let command = DeleteMessageForUserCommand {
            message_ids: vec![req.message_id],
            user_id: req.user_id,
            session_id: String::new(), // TODO: 从消息中获取或从请求中获取
            permanent: req.permanent,
        };

        match self.command_handler.handle_delete_message_for_user(command).await {
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

    async fn clear_session(
        &self,
        request: Request<ClearSessionRequest>,
    ) -> Result<Response<ClearSessionResponse>, Status> {
        let req = request.into_inner();
        
        // 解析 clear_before_time
        let clear_before_time = match req.clear_type() {
            flare_proto::storage::ClearType::ClearAll => None,
            flare_proto::storage::ClearType::ClearBeforeMessage => {
                // 需要先查询消息的时间戳
                if req.clear_before_message_id.is_empty() {
                    return Err(Status::invalid_argument("clear_before_message_id is required"));
                }
                // TODO: 查询消息时间戳
                None
            }
            flare_proto::storage::ClearType::ClearBeforeTime => {
                req.clear_before_time.as_ref().and_then(|ts| {
                    chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32)
                })
            }
        };
        
        let command = ClearSessionCommand {
            session_id: req.session_id,
            user_id: Some(req.user_id),
            clear_before_time,
        };

        match self.command_handler.handle_clear_session(command).await {
            Ok(cleared_count) => {
                let cleared_at = Utc::now();
                Ok(Response::new(ClearSessionResponse {
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
        let req = request.into_inner();
        let command = MarkReadCommand {
            message_id: req.message_id,
            user_id: req.user_id,
        };

        match self.command_handler.handle_mark_read(command).await {
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
            limit: req
                .pagination
                .as_ref()
                .map(|p| p.limit)
                .unwrap_or(200),
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
        let command = SetMessageAttributesCommand {
            message_id: req.message_id,
            attributes: req.attributes.into_iter().collect(),
            tags: req.tags,
        };

        match self.command_handler.handle_set_message_attributes(command).await {
            Ok(()) => Ok(Response::new(SetMessageAttributesResponse {
                status: Some(flare_server_core::error::ok_status()),
            })),
            Err(err) => {
                error!(error = ?err, "Failed to set message attributes");
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
            session_id: req.session_id,
            start_time: req.time_range.as_ref().and_then(|tr| {
                tr.start_time.as_ref().map(|ts| ts.seconds)
            }),
            end_time: req.time_range.as_ref().and_then(|tr| {
                tr.end_time.as_ref().map(|ts| ts.seconds)
            }),
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
}
