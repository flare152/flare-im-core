//! 标记消息已读命令服务

use anyhow::Result;
use chrono::Utc;
use flare_proto::storage::{MarkMessageReadRequest, MarkMessageReadResponse};
use prost_types::Timestamp;
use std::sync::Arc;

use crate::domain::{MessageStorage, MessageUpdate};
use crate::infrastructure::persistence::mongo::MongoMessageStorage;

pub struct MarkReadService {
    storage: Arc<MongoMessageStorage>,
}

impl MarkReadService {
    pub fn new(storage: Arc<MongoMessageStorage>) -> Self {
        Self { storage }
    }

    pub async fn execute(&self, req: MarkMessageReadRequest) -> Result<MarkMessageReadResponse> {
        if req.message_id.is_empty() {
            return Ok(MarkMessageReadResponse {
                success: false,
                error_message: "message_id is required".to_string(),
                read_at: None,
                burned_at: None,
                status: Some(flare_server_core::error::ok_status()),
            });
        }

        if req.user_id.is_empty() {
            return Ok(MarkMessageReadResponse {
                success: false,
                error_message: "user_id is required".to_string(),
                read_at: None,
                burned_at: None,
                status: Some(flare_server_core::error::ok_status()),
            });
        }

        // 获取消息
        let message = match self.storage.get_message(&req.message_id).await {
            Ok(Some(msg)) => msg,
            Ok(None) => {
                return Ok(MarkMessageReadResponse {
                    success: false,
                    error_message: "message not found".to_string(),
                    read_at: None,
                    burned_at: None,
                    status: Some(flare_server_core::error::ok_status()),
                });
            }
            Err(err) => {
                return Ok(MarkMessageReadResponse {
                    success: false,
                    error_message: format!("Failed to get message: {}", err),
                    read_at: None,
                    burned_at: None,
                    status: Some(flare_server_core::error::ok_status()),
                });
            }
        };

        let now = Utc::now();
        let read_timestamp = Timestamp {
            seconds: now.timestamp(),
            nanos: now.timestamp_subsec_nanos() as i32,
        };

        // 检查是否是阅后即焚消息
        let burned_at = if message.is_burn_after_read {
            let burn_seconds = message.burn_after_seconds as i64;
            let burn_timestamp = Timestamp {
                seconds: now.timestamp() + burn_seconds,
                nanos: now.timestamp_subsec_nanos() as i32,
            };
            Some(burn_timestamp)
        } else {
            None
        };

        // 更新已读记录
        let mut read_by = message.read_by.clone();
        let read_record = flare_proto::storage::MessageReadRecord {
            user_id: req.user_id.clone(),
            read_at: Some(read_timestamp.clone()),
            burned_at: burned_at.clone(),
        };

        // 检查是否已存在该用户的已读记录
        if let Some(existing) = read_by.iter_mut().find(|r| r.user_id == req.user_id) {
            existing.read_at = Some(read_timestamp.clone());
            existing.burned_at = burned_at.clone();
        } else {
            read_by.push(read_record);
        }

        let update = MessageUpdate {
            is_recalled: None,
            recalled_at: None,
            visibility: None,
            read_by: Some(read_by),
            operations: None,
            attributes: None,
            tags: None,
        };

        match self.storage.update_message(&req.message_id, update).await {
            Ok(()) => Ok(MarkMessageReadResponse {
                success: true,
                error_message: String::new(),
                read_at: Some(read_timestamp),
                burned_at,
                status: Some(flare_server_core::error::ok_status()),
            }),
            Err(err) => {
                tracing::error!(error = ?err, message_id = %req.message_id, user_id = %req.user_id, "Failed to mark message as read");
                Ok(MarkMessageReadResponse {
                    success: false,
                    error_message: format!("Failed to mark message as read: {}", err),
                    read_at: None,
                    burned_at: None,
                    status: Some(flare_server_core::error::ok_status()),
                })
            }
        }
    }
}

