//! 查询处理器（编排层）- 轻量级，只负责编排领域服务

use std::sync::Arc;

use flare_im_core::error::Result;
use tracing::instrument;

use crate::application::queries::{QueryMessageQuery, QueryMessagesQuery, SearchMessagesQuery};
use crate::domain::service::MessageDomainService;

/// 消息查询处理器（编排层）
/// 
/// 注意：当前查询操作主要转发到 StorageReaderService，
/// 如果未来需要在 orchestrator 中实现查询逻辑，可以在这里添加
pub struct MessageQueryHandler {
    domain_service: Arc<MessageDomainService>,
}

impl MessageQueryHandler {
    pub fn new(domain_service: Arc<MessageDomainService>) -> Self {
        Self { domain_service }
    }

    /// 查询单条消息
    /// 
    /// 当前实现：转发到 StorageReaderService
    /// 未来可以在这里添加缓存逻辑等
    #[instrument(skip(self), fields(message_id = %query.message_id, session_id = %query.session_id))]
    pub async fn query_message(&self, query: QueryMessageQuery) -> Result<()> {
        // TODO: 实现查询逻辑或转发到 StorageReaderService
        todo!("Query message logic to be implemented")
    }

    /// 查询消息列表
    #[instrument(skip(self), fields(session_id = %query.session_id))]
    pub async fn query_messages(&self, query: QueryMessagesQuery) -> Result<()> {
        // TODO: 实现查询逻辑或转发到 StorageReaderService
        todo!("Query messages logic to be implemented")
    }

    /// 搜索消息
    #[instrument(skip(self), fields(keyword = %query.keyword))]
    pub async fn search_messages(&self, query: SearchMessagesQuery) -> Result<()> {
        // TODO: 实现搜索逻辑或转发到 StorageReaderService
        todo!("Search messages logic to be implemented")
    }
}

