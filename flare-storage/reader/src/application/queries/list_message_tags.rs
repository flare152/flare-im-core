//! 列出消息标签查询服务

use anyhow::Result;
use flare_proto::storage::{ListMessageTagsRequest, ListMessageTagsResponse};
use std::sync::Arc;

use crate::domain::MessageStorage;
use crate::infrastructure::persistence::mongo::MongoMessageStorage;

pub struct ListMessageTagsService {
    storage: Arc<MongoMessageStorage>,
}

impl ListMessageTagsService {
    pub fn new(storage: Arc<MongoMessageStorage>) -> Self {
        Self { storage }
    }

    pub async fn execute(&self, _req: ListMessageTagsRequest) -> Result<ListMessageTagsResponse> {
        match self.storage.list_all_tags().await {
            Ok(tags) => Ok(ListMessageTagsResponse {
                tags,
                status: Some(flare_server_core::error::ok_status()),
            }),
            Err(err) => {
                tracing::error!(error = ?err, "Failed to list message tags");
                Ok(ListMessageTagsResponse {
                    tags: vec![],
                    status: Some(flare_server_core::error::ok_status()),
                })
            }
        }
    }
}

