//! 领域模型

use chrono::{DateTime, Utc};

/// 会话模型
#[derive(Clone, Debug)]
pub struct Session {
    pub session_id: String,
    pub user_id: String,
    pub device_id: String,
    pub route_server: Option<String>,
    pub gateway_id: String,
    pub connection_id: Option<String>,
    pub last_heartbeat: DateTime<Utc>,
}

impl Session {
    pub fn new(
        session_id: String,
        user_id: String,
        device_id: String,
        route_server: Option<String>,
        gateway_id: String,
    ) -> Self {
        Self {
            session_id,
            user_id,
            device_id,
            route_server,
            gateway_id,
            connection_id: None,
            last_heartbeat: Utc::now(),
        }
    }

    pub fn touch(&mut self) {
        self.last_heartbeat = Utc::now();
    }

    pub fn set_connection(&mut self, connection_id: Option<String>) {
        self.connection_id = connection_id;
    }
}

/// 连接信息模型
#[derive(Clone, Debug)]
pub struct ConnectionInfo {
    pub connection_id: String,
    pub protocol: String,
    pub device_id: String,
    pub platform: String,
    pub connected_at: Option<DateTime<Utc>>,
    pub last_active_at: Option<DateTime<Utc>>,
}
