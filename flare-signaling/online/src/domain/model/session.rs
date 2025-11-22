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

