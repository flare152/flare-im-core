use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 设备信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub device_id: String,
    pub platform: String,
    pub model: Option<String>,
    pub os_version: Option<String>,
    pub last_active_time: DateTime<Utc>,
}

/// 用户在线状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPresence {
    pub user_id: String,
    pub is_online: bool,
    pub devices: Vec<DeviceInfo>,
    pub last_seen: DateTime<Utc>,
}
