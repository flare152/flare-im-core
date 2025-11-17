//! 获取单条消息查询服务

use anyhow::Result;
use flare_proto::storage::{GetMessageRequest, GetMessageResponse};
use std::sync::Arc;

use crate::domain::MessageStorage;
use crate::infrastructure::persistence::mongo::MongoMessageStorage;

pub struct GetMessageService {
    storage: Arc<MongoMessageStorage>,
}

impl GetMessageService {
    pub fn new(storage: Arc<MongoMessageStorage>) -> Self {
        Self { storage }
    }

    pub async fn execute(&self, req: GetMessageRequest) -> Result<GetMessageResponse> {
        if req.message_id.is_empty() {
            return Ok(GetMessageResponse {
                message: None,
                status: Some(flare_server_core::error::ok_status()),
            });
        }

        match self.storage.get_message(&req.message_id).await {
            Ok(Some(message)) => Ok(GetMessageResponse {
                message: Some(message),
                status: Some(flare_server_core::error::ok_status()),
            }),
            Ok(None) => Ok(GetMessageResponse {
                message: None,
                status: Some(flare_server_core::error::ok_status()),
            }),
            Err(err) => {
                tracing::error!(error = ?err, message_id = %req.message_id, "Failed to get message");
                Ok(GetMessageResponse {
                    message: None,
                status: Some(flare_server_core::error::ok_status()),
                })
            }
        }
    }
}

