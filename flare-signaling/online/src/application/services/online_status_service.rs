use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use flare_proto::signaling::{
    DeviceConflictStrategy, GetOnlineStatusResponse, HeartbeatResponse, LoginRequest,
    LoginResponse, LogoutRequest, LogoutResponse, OnlineStatus,
};
use flare_server_core::error::Result;
use prost_types::Timestamp;
use tokio::sync::RwLock;
use tracing::{info, warn};
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
        let user_id = &request.user_id;
        let device_id = &request.device_id;
        let device_platform = request.device_platform.as_str();
        let desired_strategy = request.desired_conflict_strategy();
        let applied_strategy = desired_strategy;

        // 检查现有会话
        let existing_sessions = self.repository.get_user_sessions(user_id).await?;

        // 根据冲突策略处理现有会话
        if !existing_sessions.is_empty() {
            match applied_strategy {
                DeviceConflictStrategy::Exclusive => {
                    // 互斥：踢出所有旧设备
                    info!(
                        user_id = %user_id,
                        device_id = %device_id,
                        "Exclusive strategy: removing all existing sessions"
                    );
                    self.repository
                        .remove_user_sessions(user_id, None)
                        .await?;
                }
                DeviceConflictStrategy::PlatformExclusive => {
                    // 平台互斥：只踢出同平台的旧设备
                    let same_platform_devices: Vec<String> = existing_sessions
                        .iter()
                        .filter(|s| s.device_platform == device_platform)
                        .map(|s| s.device_id.clone())
                        .collect();
                    if !same_platform_devices.is_empty() {
                        info!(
                            user_id = %user_id,
                            device_id = %device_id,
                            platform = %device_platform,
                            "Platform exclusive strategy: removing same platform devices"
                        );
                        self.repository
                            .remove_user_sessions(user_id, Some(&same_platform_devices))
                            .await?;
                    }
                }
                DeviceConflictStrategy::Coexist => {
                    // 共存：允许多设备同时在线
                    info!(
                        user_id = %user_id,
                        device_id = %device_id,
                        "Coexist strategy: allowing multiple devices"
                    );
                }
                _ => {
                    // 未指定策略，默认使用互斥
                    warn!(
                        user_id = %user_id,
                        "No conflict strategy specified, using Exclusive"
                    );
                    self.repository
                        .remove_user_sessions(user_id, None)
                        .await?;
                }
            }
        }

        // 从 metadata 中提取 gateway_id（用于跨地区路由）
        // 如果 metadata 中没有 gateway_id，使用配置的默认值
        let gateway_id = request.metadata
            .get("gateway_id")
            .map(|s| s.clone())
            .unwrap_or_else(|| self.gateway_id.clone());
        
        // 创建新会话
        let session_id = Uuid::new_v4().to_string();
        let record = SessionRecord {
            session_id: session_id.clone(),
            user_id: user_id.clone(),
            device_id: device_id.clone(),
            device_platform: device_platform.to_string(),
            server_id: request.server_id.clone(),
            gateway_id: gateway_id.clone(),
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

        info!(
            user_id = %user_id,
            session_id = %session_id,
            device_id = %device_id,
            gateway_id = %gateway_id,
            "User logged in successfully"
        );

        Ok(LoginResponse {
            success: true,
            session_id,
            route_server: request.server_id,
            error_message: String::new(),
            status: util::rpc_status_ok(),
            applied_conflict_strategy: applied_strategy as i32,
        })
    }

    pub async fn logout(&self, request: LogoutRequest) -> Result<LogoutResponse> {
        let user_id = &request.user_id;
        let session_id = &request.session_id;

        // 从内存中移除会话
        {
            let mut map = self.sessions.write().await;
            map.remove(session_id);
        }

        // 从Redis中移除会话
        self.repository
            .remove_session(session_id, user_id)
            .await?;

        info!(
            user_id = %user_id,
            session_id = %session_id,
            "User logged out successfully"
        );

        Ok(LogoutResponse {
            success: true,
            status: util::rpc_status_ok(),
        })
    }

    pub async fn heartbeat(&self, session_id: &str, user_id: &str) -> Result<HeartbeatResponse> {
        // 检查会话是否存在
        {
            let map = self.sessions.read().await;
            if !map.contains_key(session_id) {
                return Ok(HeartbeatResponse {
                    success: false,
                    status: util::rpc_status_error(
                        flare_server_core::error::ErrorCode::InvalidParameter,
                        "Session not found",
                    ),
                });
            }
        }

        // 更新内存中的last_seen
        {
            let mut map = self.sessions.write().await;
            if let Some(session) = map.get_mut(session_id) {
                session.record.last_seen = Utc::now();
            }
        }

        // 更新Redis中的会话TTL
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
                    device_id: None,
                    device_platform: None,
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
                    device_id: status.device_id.unwrap_or_default(),
                    device_platform: status.device_platform.unwrap_or_default(),
                    gateway_id: status.gateway_id.unwrap_or_default(), // 返回 gateway_id 用于跨地区路由
                },
            );
        }

        Ok(GetOnlineStatusResponse {
            statuses: result,
            status: util::rpc_status_ok(),
        })
    }
}
