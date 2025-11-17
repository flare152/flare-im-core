//! 清理会话命令服务

use anyhow::Result;
use chrono::{DateTime, Utc};
use flare_proto::storage::{ClearSessionRequest, ClearSessionResponse, ClearType};
use prost_types::Timestamp;
use std::sync::Arc;

use crate::domain::{MessageStorage, MessageUpdate};
use crate::infrastructure::persistence::mongo::MongoMessageStorage;

pub struct ClearSessionService {
    storage: Arc<MongoMessageStorage>,
}

impl ClearSessionService {
    pub fn new(storage: Arc<MongoMessageStorage>) -> Self {
        Self { storage }
    }

    pub async fn execute(&self, req: ClearSessionRequest) -> Result<ClearSessionResponse> {
        if req.session_id.is_empty() {
            return Ok(ClearSessionResponse {
                success: false,
                error_message: "session_id is required".to_string(),
                cleared_count: 0,
                cleared_at: None,
                status: Some(flare_server_core::error::ok_status()),
            });
        }

        let clear_before_time = match req.clear_type() {
            ClearType::ClearAll => None,
            ClearType::ClearBeforeMessage => {
                // 需要先查询消息的时间戳
                if req.clear_before_message_id.is_empty() {
                    return Ok(ClearSessionResponse {
                        success: false,
                        error_message: "clear_before_message_id is required".to_string(),
                        cleared_count: 0,
                        cleared_at: None,
                        status: Some(flare_server_core::error::ok_status()),
                    });
                }
                match self.storage.get_message(&req.clear_before_message_id).await {
                    Ok(Some(msg)) => msg.timestamp.as_ref().and_then(|ts| {
                        DateTime::from_timestamp(ts.seconds, ts.nanos as u32)
                    }),
                    _ => None,
                }
            }
            ClearType::ClearBeforeTime => req.clear_before_time.as_ref().and_then(|ts| {
                DateTime::from_timestamp(ts.seconds, ts.nanos as u32)
            }),
        };

        // 查询需要清理的消息
        let messages = self
            .storage
            .query_messages(
                &req.session_id,
                Some(&req.user_id),
                None,
                clear_before_time,
                10000, // 最大清理数量
            )
            .await
            .map_err(|e| anyhow::anyhow!("Failed to query messages: {}", e))?;

        let cleared_count = messages.len() as i32;
        let cleared_at = Utc::now();
        let cleared_timestamp = Some(Timestamp {
            seconds: cleared_at.timestamp(),
            nanos: cleared_at.timestamp_subsec_nanos() as i32,
        });

        // 批量更新 visibility 为 DELETED
        let message_ids: Vec<String> = messages.iter().map(|m| m.id.clone()).collect();
        let _ = self
            .storage
            .batch_update_visibility(
                &message_ids,
                &req.user_id,
                flare_proto::storage::VisibilityStatus::Deleted,
            )
            .await;

        Ok(ClearSessionResponse {
            success: true,
            error_message: String::new(),
            cleared_count,
            cleared_at: cleared_timestamp,
            status: Some(flare_server_core::error::ok_status()),
        })
    }
}

