//! 设置消息属性命令服务

use anyhow::Result;
use flare_proto::storage::{SetMessageAttributesRequest, SetMessageAttributesResponse};
use std::collections::HashMap;
use std::sync::Arc;

use crate::domain::MessageStorage;
use crate::infrastructure::persistence::mongo::MongoMessageStorage;

pub struct SetMessageAttributesService {
    storage: Arc<MongoMessageStorage>,
}

impl SetMessageAttributesService {
    pub fn new(storage: Arc<MongoMessageStorage>) -> Self {
        Self { storage }
    }

    pub async fn execute(&self, req: SetMessageAttributesRequest) -> Result<SetMessageAttributesResponse> {
        if req.message_id.is_empty() {
            return Ok(SetMessageAttributesResponse {
                status: Some(flare_server_core::error::ok_status()),
            });
        }

        // 转换 attributes
        let attributes: HashMap<String, String> = req.attributes.into_iter().collect();

        // 转换 tags
        let tags = req.tags;

        match self
            .storage
            .update_message_attributes(&req.message_id, attributes, tags)
            .await
        {
            Ok(()) => Ok(SetMessageAttributesResponse {
                status: Some(flare_server_core::error::ok_status()),
            }),
            Err(err) => {
                tracing::error!(error = ?err, message_id = %req.message_id, "Failed to set message attributes");
                Ok(SetMessageAttributesResponse {
                    status: Some(flare_server_core::error::ok_status()),
                })
            }
        }
    }
}

