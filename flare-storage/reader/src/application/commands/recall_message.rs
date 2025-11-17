//! 撤回消息命令服务

use anyhow::Result;
use chrono::Utc;
use flare_proto::storage::{RecallMessageRequest, RecallMessageResponse};
use prost_types::Timestamp;
use std::sync::Arc;

use crate::domain::{MessageStorage, MessageUpdate};
use crate::infrastructure::persistence::mongo::MongoMessageStorage;

pub struct RecallMessageService {
    storage: Arc<MongoMessageStorage>,
    default_recall_time_limit_seconds: i64,
}

impl RecallMessageService {
    pub fn new(storage: Arc<MongoMessageStorage>) -> Self {
        Self {
            storage,
            default_recall_time_limit_seconds: 2 * 60, // 默认2分钟
        }
    }

    pub async fn execute(&self, req: RecallMessageRequest) -> Result<RecallMessageResponse> {
        if req.message_id.is_empty() {
            return Ok(RecallMessageResponse {
                success: false,
                error_message: "message_id is required".to_string(),
                recalled_at: None,
                status: Some(flare_server_core::error::ok_status()),
            });
        }

        // 检查消息是否存在
        let message = match self.storage.get_message(&req.message_id).await {
            Ok(Some(msg)) => msg,
            Ok(None) => {
                return Ok(RecallMessageResponse {
                    success: false,
                    error_message: "message not found".to_string(),
                    recalled_at: None,
                    status: Some(flare_server_core::error::ok_status()),
                });
            }
            Err(err) => {
                return Ok(RecallMessageResponse {
                    success: false,
                    error_message: format!("Failed to get message: {}", err),
                    recalled_at: None,
                    status: Some(flare_server_core::error::ok_status()),
                });
            }
        };

        // 检查撤回时间限制
        let recall_time_limit: i64 = if req.recall_time_limit_seconds > 0 {
            req.recall_time_limit_seconds as i64
        } else {
            self.default_recall_time_limit_seconds
        };

        let message_timestamp = message
            .timestamp
            .as_ref()
            .map(|ts| ts.seconds)
            .unwrap_or(0);
        let now = Utc::now().timestamp();
        let elapsed = now - message_timestamp;

        if elapsed > recall_time_limit {
            return Ok(RecallMessageResponse {
                success: false,
                error_message: format!(
                    "Message is too old to recall (limit: {}s, elapsed: {}s)",
                    recall_time_limit, elapsed
                ),
                recalled_at: None,
                status: Some(flare_server_core::error::ok_status()),
            });
        }

        // 执行撤回
        let recalled_at = Utc::now();
        let recalled_timestamp = Some(Timestamp {
            seconds: recalled_at.timestamp(),
            nanos: recalled_at.timestamp_subsec_nanos() as i32,
        });

        let update = MessageUpdate {
            is_recalled: Some(true),
            recalled_at: recalled_timestamp.clone(),
            visibility: None,
            read_by: None,
            operations: None,
            attributes: None,
            tags: None,
        };

        match self.storage.update_message(&req.message_id, update).await {
            Ok(()) => Ok(RecallMessageResponse {
                success: true,
                error_message: String::new(),
                recalled_at: recalled_timestamp,
                status: Some(flare_server_core::error::ok_status()),
            }),
            Err(err) => {
                tracing::error!(error = ?err, message_id = %req.message_id, "Failed to recall message");
                Ok(RecallMessageResponse {
                    success: false,
                    error_message: format!("Failed to recall message: {}", err),
                    recalled_at: None,
                    status: Some(flare_server_core::error::ok_status()),
                })
            }
        }
    }
}

