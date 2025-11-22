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

