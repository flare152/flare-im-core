//! 为用户删除消息命令服务（软删除）

use anyhow::Result;
use flare_proto::storage::{DeleteMessageForUserRequest, DeleteMessageForUserResponse};
use std::sync::Arc;

use crate::domain::{MessageStorage, VisibilityStorage};
use crate::infrastructure::persistence::mongo::MongoMessageStorage;

pub struct DeleteMessageForUserService {
    storage: Arc<MongoMessageStorage>,
}

impl DeleteMessageForUserService {
    pub fn new(storage: Arc<MongoMessageStorage>) -> Self {
        Self { storage }
    }

    pub async fn execute(&self, req: DeleteMessageForUserRequest) -> Result<DeleteMessageForUserResponse> {
        if req.message_id.is_empty() {
            return Ok(DeleteMessageForUserResponse {
                success: false,
                deleted_count: 0,
                status: Some(flare_server_core::error::ok_status()),
            });
        }

        if req.user_id.is_empty() {
            return Ok(DeleteMessageForUserResponse {
                success: false,
                deleted_count: 0,
                status: Some(flare_server_core::error::ok_status()),
            });
        }

        // 检查消息是否存在
        let message = match self.storage.get_message(&req.message_id).await {
            Ok(Some(msg)) => msg,
            Ok(None) => {
                return Ok(DeleteMessageForUserResponse {
                    success: false,
                    deleted_count: 0,
                    status: Some(flare_server_core::error::ok_status()),
                });
            }
            Err(err) => {
                tracing::error!(error = ?err, message_id = %req.message_id, "Failed to get message");
                return Ok(DeleteMessageForUserResponse {
                    success: false,
                    deleted_count: 0,
                    status: Some(flare_server_core::error::ok_status()),
                });
            }
        };

        // 软删除：更新 visibility 为 HIDDEN（permanent=false）或 DELETED（permanent=true）
        let visibility = if req.permanent {
            flare_proto::storage::VisibilityStatus::Deleted
        } else {
            flare_proto::storage::VisibilityStatus::Hidden
        };

        match self
            .storage
            .batch_set_visibility(
                &[req.message_id.clone()],
                &req.user_id,
                &message.session_id,
                visibility,
            )
            .await
        {
            Ok(count) => Ok(DeleteMessageForUserResponse {
                success: true,
                deleted_count: count as i32,
                status: Some(flare_server_core::error::ok_status()),
            }),
            Err(err) => {
                tracing::error!(error = ?err, message_id = %req.message_id, user_id = %req.user_id, "Failed to delete message for user");
                Ok(DeleteMessageForUserResponse {
                    success: false,
                    deleted_count: 0,
                    status: Some(flare_server_core::error::ok_status()),
                })
            }
        }
    }
}

