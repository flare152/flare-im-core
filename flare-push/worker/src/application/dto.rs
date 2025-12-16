//! 数据传输对象（DTO）定义

use serde::{Deserialize, Serialize};

/// 推送统计响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushStatsResponse {
    /// 总推送数
    pub total_pushes: u64,
    /// 成功推送数
    pub successful_pushes: u64,
    /// 失败推送数
    pub failed_pushes: u64,
    /// 平均延迟（毫秒）
    pub average_latency_ms: f64,
    /// P99延迟（毫秒）
    pub p99_latency_ms: f64,
}
