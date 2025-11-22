use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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

/// 在线状态变化事件
#[derive(Debug, Clone)]
pub struct PresenceChangeEvent {
    pub user_id: String,
    pub status: OnlineStatusRecord,
    pub occurred_at: DateTime<Utc>,
    pub conflict_action: Option<i32>, // ConflictAction enum value
    pub reason: Option<String>,
}

