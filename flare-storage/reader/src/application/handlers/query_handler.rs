//! 查询处理器（查询侧）- 直接调用基础设施层，不经过领域服务
//!
//! 在 CQRS 架构中，查询侧通常直接调用基础设施层（仓储实现），
//! 因为查询是只读操作，不涉及业务逻辑，不需要经过领域层。

use std::sync::Arc;
use anyhow::Result;
use tracing::instrument;
use flare_proto::common::Message;
use chrono::{DateTime, TimeZone, Utc};

use crate::application::queries::{
    GetMessageQuery, ListMessageTagsQuery, QueryMessagesQuery, SearchMessagesQuery,
};
use crate::domain::repository::MessageStorage;

/// 消息存储查询处理器（查询侧）
/// 
/// 直接调用基础设施层的仓储实现，不经过领域服务
pub struct MessageStorageQueryHandler {
    storage: Arc<dyn MessageStorage + Send + Sync>,
}

impl MessageStorageQueryHandler {
    pub fn new(storage: Arc<dyn MessageStorage + Send + Sync>) -> Self {
        Self { storage }
    }

    /// 查询消息列表
    #[instrument(skip(self), fields(session_id = %query.session_id))]
    pub async fn handle_query_messages(
        &self,
        query: QueryMessagesQuery,
    ) -> Result<Vec<Message>> {
        let start_time = if query.start_time == 0 {
            None
        } else {
            DateTime::from_timestamp(query.start_time, 0)
        };
        
        let end_time = if query.end_time == 0 {
            None
        } else {
            DateTime::from_timestamp(query.end_time, 0)
        };

        self.storage
            .query_messages(
                &query.session_id,
                None, // user_id
                start_time,
                end_time,
                query.limit,
            )
            .await
    }

    /// 获取单条消息
    #[instrument(skip(self), fields(message_id = %query.message_id))]
    pub async fn handle_get_message(&self, query: GetMessageQuery) -> Result<Option<Message>> {
        self.storage.get_message(&query.message_id).await
    }

    /// 搜索消息
    #[instrument(skip(self))]
    pub async fn handle_search_messages(
        &self,
        query: SearchMessagesQuery,
    ) -> Result<Vec<Message>> {
        let start_time = if query.start_time == 0 {
            None
        } else {
            DateTime::from_timestamp(query.start_time, 0)
        };
        
        let end_time = if query.end_time == 0 {
            None
        } else {
            DateTime::from_timestamp(query.end_time, 0)
        };

        self.storage
            .search_messages(
                &query.filters,
                start_time,
                end_time,
                query.limit,
            )
            .await
    }

    /// 列出所有标签
    #[instrument(skip(self))]
    pub async fn handle_list_message_tags(
        &self,
        _query: ListMessageTagsQuery,
    ) -> Result<Vec<String>> {
        self.storage.list_all_tags().await
    }
}

