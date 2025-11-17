use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub session_id: String,
    pub user_id: String,
    pub device_id: String,
    pub device_platform: String,
    pub server_id: String,
    pub gateway_id: String,
    pub last_seen: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineStatusRecord {
    pub online: bool,
    pub server_id: String,
    pub gateway_id: Option<String>,
    pub cluster_id: Option<String>,
    pub last_seen: Option<DateTime<Utc>>,
    pub device_id: Option<String>,
    pub device_platform: Option<String>,
}

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
