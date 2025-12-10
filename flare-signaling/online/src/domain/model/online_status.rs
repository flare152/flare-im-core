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
