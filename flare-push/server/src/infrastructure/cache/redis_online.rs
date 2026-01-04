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
use crate::infrastructure::session_client::ConversationServiceClient;
use crate::infrastructure::signaling::SignalingOnlineClient;

/// 在线状态仓储 - 直接使用 Signaling Online 服务
pub struct OnlineStatusRepositoryImpl {
    signaling_client: Arc<SignalingOnlineClient>,
    conversation_client: Option<Arc<ConversationServiceClient>>,
    default_tenant_id: String,
}

impl OnlineStatusRepositoryImpl {
    pub fn new(signaling_client: Arc<SignalingOnlineClient>, default_tenant_id: String) -> Self {
        Self {
            signaling_client,
            conversation_client: None,
            default_tenant_id,
        }
    }

    pub fn with_conversation_client(
        signaling_client: Arc<SignalingOnlineClient>,
        conversation_client: Arc<ConversationServiceClient>,
        default_tenant_id: String,
    ) -> Self {
        Self {
            signaling_client,
            conversation_client: Some(conversation_client),
            default_tenant_id,
        }
    }
}

#[async_trait]
impl OnlineStatusRepository for OnlineStatusRepositoryImpl {
    async fn is_online(&self, ctx: &flare_server_core::context::Context) -> Result<bool> {
        let user_id = ctx.user_id().ok_or_else(|| {
            flare_server_core::error::ErrorBuilder::new(
                flare_server_core::error::ErrorCode::InvalidParameter,
                "user_id is required in context"
            )
            .build_error()
        })?;
        let statuses = self.batch_get_online_status(&[user_id.to_string()]).await?;

        Ok(statuses.get(user_id).map(|s| s.online).unwrap_or(false))
    }

    async fn batch_get_online_status(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, OnlineStatus>> {
        // 从 default_tenant_id 创建 Context（向后兼容）
        use flare_server_core::context::Context;
        let ctx = Context::root().with_tenant_id(self.default_tenant_id.clone());
        
        // 直接调用 Signaling Online 服务批量查询
        self.signaling_client
            .batch_get_online_status(&ctx, user_ids)
            .await
    }

    async fn get_all_online_users_for_session(&self, conversation_id: &str) -> Result<Vec<String>> {
        // 添加超时保护，避免阻塞
        let timeout_duration = std::time::Duration::from_secs(5); // 5秒超时

        // 1. 尝试通过 Hook 获取参与者（如果配置了Hook）
        // 注意：这里暂时不实现Hook调用，因为需要HookDispatcher
        // 后续可以在PushDomainService中调用Hook

        // 2. 通过 Session 服务获取会话的所有参与者（带超时，降级方案）
        use flare_server_core::context::Context;
        let ctx = Context::root().with_tenant_id(self.default_tenant_id.clone());
        
        let participant_user_ids = if let Some(ref conversation_client) = self.conversation_client {
            match tokio::time::timeout(
                timeout_duration,
                conversation_client.get_conversation_participants(&ctx, conversation_id),
            )
            .await
            {
                Ok(Ok(participants)) => {
                    if participants.is_empty() {
                        warn!(conversation_id = %conversation_id, "Session has no participants");
                        return Ok(vec![]);
                    }
                    info!(conversation_id = %conversation_id, participant_count = participants.len(), "Fetched session participants");
                    participants
                }
                Ok(Err(e)) => {
                    warn!(error = %e, conversation_id = %conversation_id, "Failed to get session participants from Session service");
                    return Err(e);
                }
                Err(_) => {
                    warn!(
                        timeout_secs = timeout_duration.as_secs(),
                        conversation_id = %conversation_id,
                        "Timeout waiting for session participants from Session service"
                    );
                    return Err(flare_server_core::error::ErrorBuilder::new(
                        flare_server_core::error::ErrorCode::ServiceUnavailable,
                        format!("Timeout waiting for session participants from Session service (timeout: {}s)", timeout_duration.as_secs())
                    ).details(format!("conversation_id: {}", conversation_id)).build_error());
                }
            }
        } else {
            return Err(flare_server_core::error::ErrorBuilder::new(
                flare_server_core::error::ErrorCode::ServiceUnavailable,
                "Session service client not configured. Cannot query session participants",
            )
            .details(format!("conversation_id: {}", conversation_id))
            .build_error());
        };

        // 2. 批量查询这些参与者的在线状态（带超时）
        let ctx = Context::root().with_tenant_id(self.default_tenant_id.clone());
        
        let online_statuses = match tokio::time::timeout(
            timeout_duration,
            self.signaling_client
                .batch_get_online_status(&ctx, &participant_user_ids),
        )
        .await
        {
            Ok(Ok(statuses)) => statuses,
            Ok(Err(e)) => {
                warn!(error = %e, conversation_id = %conversation_id, "Failed to get online status from Signaling service");
                return Err(e);
            }
            Err(_) => {
                warn!(
                    timeout_secs = timeout_duration.as_secs(),
                    conversation_id = %conversation_id,
                    "Timeout waiting for online status from Signaling service"
                );
                return Err(flare_server_core::error::ErrorBuilder::new(
                    flare_server_core::error::ErrorCode::ServiceUnavailable,
                    format!(
                        "Timeout waiting for online status from Signaling service (timeout: {}s)",
                        timeout_duration.as_secs()
                    ),
                )
                .details(format!("conversation_id: {}", conversation_id))
                .build_error());
            }
        };

        // 3. 过滤出所有在线的用户ID
        let online_user_ids: Vec<String> = online_statuses
            .into_iter()
            .filter(|(_, status)| status.online)
            .map(|(user_id, _)| user_id)
            .collect();

        info!(
            conversation_id = %conversation_id,
            total_participants = participant_user_ids.len(),
            online_count = online_user_ids.len(),
            "Filtered online users for session"
        );

        Ok(online_user_ids)
    }
}
