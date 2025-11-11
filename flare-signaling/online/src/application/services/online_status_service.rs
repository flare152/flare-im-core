use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use flare_proto::signaling::{
    GetOnlineStatusResponse, HeartbeatResponse, LoginRequest, LoginResponse, LogoutRequest,
    LogoutResponse, OnlineStatus,
};
use flare_server_core::error::Result;
use prost_types::Timestamp;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::config::OnlineConfig;
use crate::domain::entities::{OnlineStatusRecord, SessionRecord};
use crate::domain::repositories::SessionRepository;
use crate::util;

#[derive(Debug, Clone)]
struct InMemorySession {
    record: SessionRecord,
}

pub struct OnlineStatusService {
    repository: Arc<dyn SessionRepository>,
    sessions: Arc<RwLock<HashMap<String, InMemorySession>>>,
    gateway_id: String,
    #[allow(dead_code)]
    config: Arc<OnlineConfig>,
}

impl OnlineStatusService {
    pub fn new(config: Arc<OnlineConfig>, repository: Arc<dyn SessionRepository>) -> Self {
        Self {
            repository,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            gateway_id: format!("gateway-{}", &Uuid::new_v4().to_string()[..8]),
            config,
        }
    }

    pub async fn login(&self, request: LoginRequest) -> Result<LoginResponse> {
        let session_id = Uuid::new_v4().to_string();
        let record = SessionRecord {
            session_id: session_id.clone(),
            user_id: request.user_id.clone(),
            device_id: request.device_id.clone(),
            server_id: request.server_id.clone(),
            gateway_id: self.gateway_id.clone(),
            last_seen: Utc::now(),
        };

        {
            let mut map = self.sessions.write().await;
            map.insert(
                session_id.clone(),
                InMemorySession {
                    record: record.clone(),
                },
            );
        }

        self.repository.save_session(&record).await?;

        Ok(LoginResponse {
            success: true,
            session_id,
            route_server: request.server_id,
            error_message: String::new(),
            status: util::rpc_status_ok(),
        })
    }

    pub async fn logout(&self, request: LogoutRequest) -> Result<LogoutResponse> {
        {
            let mut map = self.sessions.write().await;
            map.remove(&request.session_id);
        }

        self.repository
            .remove_session(&request.session_id, &request.user_id)
            .await?;

        Ok(LogoutResponse {
            success: true,
            status: util::rpc_status_ok(),
        })
    }

    pub async fn heartbeat(&self, session_id: &str, user_id: &str) -> Result<HeartbeatResponse> {
        {
            let mut map = self.sessions.write().await;
            if let Some(session) = map.get_mut(session_id) {
                session.record.last_seen = Utc::now();
            }
        }

        self.repository.touch_session(user_id).await?;

        Ok(HeartbeatResponse {
            success: true,
            status: util::rpc_status_ok(),
        })
    }

    pub async fn get_online_status(&self, user_ids: &[String]) -> Result<GetOnlineStatusResponse> {
        let statuses = self.repository.fetch_statuses(user_ids).await?;

        let mut result = HashMap::new();
        for user_id in user_ids {
            let status = statuses
                .get(user_id)
                .cloned()
                .unwrap_or_else(|| OnlineStatusRecord {
                    online: false,
                    server_id: String::new(),
                    gateway_id: None,
                    cluster_id: None,
                    last_seen: None,
                });
            result.insert(
                user_id.clone(),
                OnlineStatus {
                    online: status.online,
                    server_id: status.server_id,
                    cluster_id: status.cluster_id.unwrap_or_default(),
                    last_seen: status.last_seen.as_ref().map(|dt| Timestamp {
                        seconds: dt.timestamp(),
                        nanos: dt.timestamp_subsec_nanos() as i32,
                    }),
                    tenant: None,
                },
            );
        }

        Ok(GetOnlineStatusResponse {
            statuses: result,
            status: util::rpc_status_ok(),
        })
    }
}
