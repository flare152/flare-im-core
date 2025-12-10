use serde::{Deserialize, Serialize};

/// 设备路由聚合（读模型侧）
/// 
/// 代表某用户的某设备当前的路由与质量信息，用于选择最优设备。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeviceRoute {
    pub user_id: String,
    pub device_id: String,
    pub gateway_id: String,
    pub server_id: String,
    pub device_priority: i32,
    pub quality_score: f64,
}

impl DeviceRoute {
    pub fn new(
        user_id: String,
        device_id: String,
        gateway_id: String,
        server_id: String,
        device_priority: i32,
        quality_score: f64,
    ) -> Self {
        Self { user_id, device_id, gateway_id, server_id, device_priority, quality_score }
    }

    /// 判断自身是否优于另一个设备（优先级优先，其次质量得分）
    pub fn is_better_than(&self, other: &DeviceRoute) -> bool {
        match self.device_priority.cmp(&other.device_priority) {
            std::cmp::Ordering::Greater => true,
            std::cmp::Ordering::Less => false,
            std::cmp::Ordering::Equal => self.quality_score >= other.quality_score,
        }
    }
}
