//! 搜索消息查询服务

use anyhow::Result;
use chrono::{DateTime, TimeZone, Utc};
use flare_proto::common::{Pagination, TimeRange};
use flare_proto::storage::{SearchMessagesRequest, SearchMessagesResponse};
use std::sync::Arc;

use crate::config::StorageReaderConfig;
use crate::domain::MessageStorage;
use crate::infrastructure::persistence::mongo::MongoMessageStorage;

pub struct SearchMessagesService {
    config: Arc<StorageReaderConfig>,
    storage: Arc<MongoMessageStorage>,
}

impl SearchMessagesService {
    pub async fn new(config: Arc<StorageReaderConfig>) -> Result<Self> {
        let storage = if let Some(url) = &config.mongo_url {
            MongoMessageStorage::new(url, &config.mongo_database)
                .await
                .unwrap_or_default()
        } else {
            MongoMessageStorage::default()
        };

        Ok(Self {
            config,
            storage: Arc::new(storage),
        })
    }

    pub async fn execute(&self, req: SearchMessagesRequest) -> Result<SearchMessagesResponse> {
        // 解析时间范围
        let (start_time, end_time) = if let Some(time_range) = &req.time_range {
            let start = time_range
                .start_time
                .as_ref()
                .and_then(|ts| chrono::TimeZone::timestamp_opt(&Utc, ts.seconds, ts.nanos as u32).single());
            let end = time_range
                .end_time
                .as_ref()
                .and_then(|ts| chrono::TimeZone::timestamp_opt(&Utc, ts.seconds, ts.nanos as u32).single());
            (start, end)
        } else {
            let end = Some(Utc::now());
            let start = Some(Utc::now() - chrono::Duration::seconds(self.config.default_range_seconds));
            (start, end)
        };

        // 获取限制数量
        let limit = req
            .pagination
            .as_ref()
            .map(|p| p.limit)
            .unwrap_or(self.config.max_page_size)
            .clamp(1, self.config.max_page_size) as i32;

        // 执行搜索
        let messages = self
            .storage
            .search_messages(&req.filters, start_time, end_time, limit)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to search messages: {}", e))?;

        // 构建分页信息
        let pagination = req.pagination.clone().map(|mut p| {
            p.has_more = messages.len() as i32 >= limit;
            p
        });

        Ok(SearchMessagesResponse {
            messages,
            pagination,
            status: Some(flare_server_core::error::ok_status()),
        })
    }
}

