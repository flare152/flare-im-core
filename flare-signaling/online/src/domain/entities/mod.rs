use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub session_id: String,
    pub user_id: String,
    pub device_id: String,
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
}
