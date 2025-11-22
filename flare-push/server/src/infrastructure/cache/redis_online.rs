//! 在线状态仓储实现 - 直接使用 Signaling Online 服务
//! 
//! 设计原则：
//! - 直接调用 signaling-online 服务，不通过 Redis 缓存
//! - 聊天室作为超大群处理，业务系统提供所有成员列表
//! - 代码精简，提高执行效率

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use flare_server_core::error::Result;
use tracing::{info, warn};

use crate::domain::repository::{OnlineStatus, OnlineStatusRepository};
use crate::infrastructure::signaling::SignalingOnlineClient;
use crate::infrastructure::session_client::SessionServiceClient;

/// 在线状态仓储 - 直接使用 Signaling Online 服务
pub struct OnlineStatusRepositoryImpl {
    signaling_client: Arc<SignalingOnlineClient>,
    session_client: Option<Arc<SessionServiceClient>>,
    default_tenant_id: String,
}

impl OnlineStatusRepositoryImpl {
    pub fn new(
        signaling_client: Arc<SignalingOnlineClient>,
        default_tenant_id: String,
    ) -> Self {
        Self {
            signaling_client,
            session_client: None,
            default_tenant_id,
        }
    }

    pub fn with_session_client(
        signaling_client: Arc<SignalingOnlineClient>,
        session_client: Arc<SessionServiceClient>,
        default_tenant_id: String,
    ) -> Self {
        Self {
            signaling_client,
            session_client: Some(session_client),
            default_tenant_id,
        }
    }
}

#[async_trait]
impl OnlineStatusRepository for OnlineStatusRepositoryImpl {
    async fn is_online(&self, user_id: &str) -> Result<bool> {
        let statuses = self
            .batch_get_online_status(&[user_id.to_string()])
            .await?;
        
        Ok(statuses
            .get(user_id)
            .map(|s| s.online)
            .unwrap_or(false))
    }
    
    async fn batch_get_online_status(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, OnlineStatus>> {
        // 直接调用 Signaling Online 服务批量查询
        self.signaling_client
            .batch_get_online_status(user_ids, Some(&self.default_tenant_id))
            .await
    }
    
    async fn get_all_online_users_for_session(&self, session_id: &str) -> Result<Vec<String>> {
        // 添加超时保护，避免阻塞
        let timeout_duration = std::time::Duration::from_secs(5); // 5秒超时
        
        // 1. 通过 Session 服务获取会话的所有参与者（带超时）
        let participant_user_ids = if let Some(ref session_client) = self.session_client {
            match tokio::time::timeout(
                timeout_duration,
                session_client.get_session_participants(session_id, Some(&self.default_tenant_id))
            ).await {
                Ok(Ok(participants)) => {
                    if participants.is_empty() {
                        warn!(session_id = %session_id, "Session has no participants");
                        return Ok(vec![]);
                    }
                    info!(session_id = %session_id, participant_count = participants.len(), "Fetched session participants");
                    participants
                },
                Ok(Err(e)) => {
                    warn!(error = %e, session_id = %session_id, "Failed to get session participants from Session service");
                    return Err(e);
                },
                Err(_) => {
                    warn!(
                        timeout_secs = timeout_duration.as_secs(),
                        session_id = %session_id,
                        "Timeout waiting for session participants from Session service"
                    );
                    return Err(flare_server_core::error::ErrorBuilder::new(
                        flare_server_core::error::ErrorCode::ServiceUnavailable,
                        format!("Timeout waiting for session participants from Session service (timeout: {}s)", timeout_duration.as_secs())
                    ).details(format!("session_id: {}", session_id)).build_error());
                }
            }
        } else {
            return Err(flare_server_core::error::ErrorBuilder::new(
                flare_server_core::error::ErrorCode::ServiceUnavailable,
                "Session service client not configured. Cannot query session participants"
            ).details(format!("session_id: {}", session_id)).build_error());
        };

        // 2. 批量查询这些参与者的在线状态（带超时）
        let online_statuses = match tokio::time::timeout(
            timeout_duration,
            self.signaling_client.batch_get_online_status(&participant_user_ids, Some(&self.default_tenant_id))
        ).await {
            Ok(Ok(statuses)) => statuses,
            Ok(Err(e)) => {
                warn!(error = %e, session_id = %session_id, "Failed to get online status from Signaling service");
                return Err(e);
            },
            Err(_) => {
                warn!(
                    timeout_secs = timeout_duration.as_secs(),
                    session_id = %session_id,
                    "Timeout waiting for online status from Signaling service"
                );
                return Err(flare_server_core::error::ErrorBuilder::new(
                    flare_server_core::error::ErrorCode::ServiceUnavailable,
                    format!("Timeout waiting for online status from Signaling service (timeout: {}s)", timeout_duration.as_secs())
                ).details(format!("session_id: {}", session_id)).build_error());
            }
        };

        // 3. 过滤出所有在线的用户ID
        let online_user_ids: Vec<String> = online_statuses
            .into_iter()
            .filter(|(_, status)| status.online)
            .map(|(user_id, _)| user_id)
            .collect();

        info!(
            session_id = %session_id,
            total_participants = participant_user_ids.len(),
            online_count = online_user_ids.len(),
            "Filtered online users for session"
        );

        Ok(online_user_ids)
    }
}
