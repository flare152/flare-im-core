//! 查询结构体定义（Query DTO）

use serde::{Deserialize, Serialize};

/// 查询推送任务状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryPushTaskStatusQuery {
    /// 消息ID
    pub message_id: String,
    /// 用户ID
    pub user_id: String,
}

/// 查询推送统计
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryPushStatisticsQuery {
    /// 租户ID（可选）
    pub tenant_id: Option<String>,
    /// 时间范围开始（Unix 时间戳）
    pub start_time: Option<i64>,
    /// 时间范围结束（Unix 时间戳）
    pub end_time: Option<i64>,
    /// 推送渠道过滤（可选）
    pub channel: Option<String>,
}
