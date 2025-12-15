//! 查询处理器（编排层）- 轻量级，只负责编排领域服务

use std::sync::Arc;

use flare_server_core::error::Result;
use tracing::instrument;

use crate::application::queries::{QueryPushStatisticsQuery, QueryPushTaskStatusQuery};
use crate::domain::service::PushDomainService;

/// 推送查询处理器（编排层）
pub struct PushQueryHandler {
    domain_service: Arc<PushDomainService>,
}

impl PushQueryHandler {
    pub fn new(domain_service: Arc<PushDomainService>) -> Self {
        Self { domain_service }
    }

    /// 查询推送任务状态
    #[instrument(skip(self), fields(message_id = %query.message_id, user_id = %query.user_id))]
    pub async fn query_push_task_status(
        &self,
        query: QueryPushTaskStatusQuery,
    ) -> Result<String> {
        // 实现查询逻辑
        // 调用领域服务查询推送任务状态
        let status = self.domain_service.get_push_task_status(&query.message_id, &query.user_id).await?;
        Ok(status.unwrap_or_default()) // 处理Option<String>到String的转换
    }

    /// 查询推送统计
    #[instrument(skip(self))]
    pub async fn query_push_statistics(
        &self,
        query: QueryPushStatisticsQuery,
    ) -> Result<crate::application::dto::PushStatsResponse> {
        // 实现推送统计查询逻辑
        // 调用领域服务查询推送统计信息
        let stats = self.domain_service.get_push_statistics(&query).await?;
        Ok(stats)
    }
}