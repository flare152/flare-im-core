//! 查询结构体定义（Query DTO）

use serde::{Deserialize, Serialize};

/// 查询消息状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryMessageStatusQuery {
    /// 消息ID
    pub message_id: String,
    /// 用户ID
    pub user_id: String,
}

/// 批量查询消息状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchQueryMessageStatusQuery {
    /// 消息ID和用户ID的映射
    pub message_user_pairs: Vec<(String, String)>,
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
    /// 消息类型过滤（可选）
    pub message_type: Option<String>,
}
