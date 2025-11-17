//! 删除消息命令服务

use anyhow::Result;
use chrono::Utc;
use flare_proto::storage::{DeleteMessageRequest, DeleteMessageResponse};
use std::sync::Arc;

use crate::domain::{MessageStorage, MessageUpdate};
use crate::infrastructure::persistence::mongo::MongoMessageStorage;

pub struct DeleteMessageService {
    storage: Arc<MongoMessageStorage>,
}

impl DeleteMessageService {
    pub fn new(storage: Arc<MongoMessageStorage>) -> Self {
        Self { storage }
    }

    pub async fn execute(&self, req: DeleteMessageRequest) -> Result<DeleteMessageResponse> {
        if req.message_ids.is_empty() {
            return Ok(DeleteMessageResponse {
                success: false,
                deleted_count: 0,
                status: Some(flare_server_core::error::ok_status()),
            });
        }

        let mut deleted_count = 0;
        let mut errors = Vec::new();

        for message_id in &req.message_ids {
            // 软删除：更新 visibility 为 DELETED
            match self
                .storage
                .batch_update_visibility(
                    &[message_id.clone()],
                    "", // 系统删除，不需要 user_id
                    flare_proto::storage::VisibilityStatus::Deleted,
                )
                .await
            {
                Ok(count) => {
                    deleted_count += count as i32;
                }
                Err(err) => {
                    errors.push(format!("Failed to delete message {}: {}", message_id, err));
                }
            }
        }

        let success = errors.is_empty();
        Ok(DeleteMessageResponse {
            success,
            deleted_count,
            status: Some(flare_server_core::error::ok_status()),
        })
    }
}

