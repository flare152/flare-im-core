//! 查询处理器（查询侧）- 直接调用基础设施层，不经过领域服务
//!
//! 在 CQRS 架构中，查询侧通常直接调用基础设施层（仓储实现），
//! 因为查询是只读操作，不涉及业务逻辑，不需要经过领域层。

use std::collections::HashMap;
use std::sync::Arc;

use flare_server_core::error::{ErrorBuilder, ErrorCode, Result};
use tracing::instrument;

use crate::application::queries::{
    BatchQueryMessageStatusQuery, QueryMessageStatusQuery, QueryPushStatisticsQuery,
};
use crate::infrastructure::message_state::{MessageStateTracker, MessageStatus};

/// 推送查询处理器（查询侧）
/// 
/// 直接调用基础设施层的实现，不经过领域服务
pub struct PushQueryHandler {
    state_tracker: Arc<MessageStateTracker>,
}

impl PushQueryHandler {
    pub fn new(state_tracker: Arc<MessageStateTracker>) -> Self {
        Self { state_tracker }
    }

    /// 查询消息状态（直接调用基础设施层）
    #[instrument(skip(self), fields(message_id = %query.message_id, user_id = %query.user_id))]
    pub async fn query_message_status(
        &self,
        query: QueryMessageStatusQuery,
    ) -> Result<MessageStatus> {
        self.state_tracker
            .get_status(&query.message_id, &query.user_id)
            .await
            .map(|state| state.status)
            .ok_or_else(|| {
                ErrorBuilder::new(
                    ErrorCode::InvalidParameter,
                    "Message status not found",
                )
                .build_error()
            })
    }

    /// 批量查询消息状态（直接调用基础设施层）
    #[instrument(skip(self), fields(count = query.message_user_pairs.len()))]
    pub async fn batch_query_message_status(
        &self,
        query: BatchQueryMessageStatusQuery,
    ) -> Result<HashMap<(String, String), MessageStatus>> {
        let mut result = HashMap::new();
        for (message_id, user_id) in query.message_user_pairs {
            if let Ok(status) = self.query_message_status(QueryMessageStatusQuery {
                message_id: message_id.clone(),
                user_id: user_id.clone(),
            }).await {
                result.insert((message_id, user_id), status);
            }
        }
        Ok(result)
    }

    /// 查询推送统计
    #[instrument(skip(self))]
    pub async fn query_push_statistics(
        &self,
        _query: QueryPushStatisticsQuery,
    ) -> Result<serde_json::Value> {
        // 实现统计查询逻辑
        // 从MessageStateTracker获取统计数据
        let stats = self.state_tracker.get_statistics().await;
        
        Ok(serde_json::json!({
            "total_pushes": stats.total_pushes,
            "success_rate": stats.success_rate,
            "pending_count": stats.pending_count,
            "delivered_count": stats.delivered_count,
            "failed_count": stats.failed_count,
            "average_delivery_time_ms": stats.average_delivery_time_ms,
        }))
    }
}

