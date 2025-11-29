use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;
use flare_proto::common::Message;

use crate::domain::model::{
    ConflictResolutionPolicy, DevicePresence, DeviceState, MessageSyncResult, Session,
    SessionBootstrapResult, SessionParticipant, SessionSummary,
};

#[derive(Clone, Debug)]
pub struct PresenceUpdate {
    pub user_id: String,
    pub device_id: String,
    pub device_platform: Option<String>,
    pub state: DeviceState,
    pub conflict_resolution: Option<ConflictResolutionPolicy>,
    pub notify_conflict: bool,
    pub conflict_reason: Option<String>,
}

/// Session 仓储接口（需要作为 trait 对象使用，保留 async-trait）
#[async_trait]
pub trait SessionRepository: Send + Sync {
    async fn load_bootstrap(
        &self,
        user_id: &str,
        client_cursor: &HashMap<String, i64>,
    ) -> Result<SessionBootstrapResult>;

    async fn update_cursor(&self, user_id: &str, session_id: &str, ts: i64) -> Result<()>;

    // 会话管理方法
    async fn create_session(&self, session: &Session) -> Result<()>;
    async fn get_session(&self, session_id: &str) -> Result<Option<Session>>;
    async fn update_session(&self, session: &Session) -> Result<()>;
    async fn delete_session(&self, session_id: &str, hard_delete: bool) -> Result<()>;
    async fn manage_participants(
        &self,
        session_id: &str,
        to_add: &[SessionParticipant],
        to_remove: &[String],
        role_updates: &[(String, Vec<String>)],
    ) -> Result<Vec<SessionParticipant>>;
    async fn batch_acknowledge(
        &self,
        user_id: &str,
        cursors: &[(String, i64)],
    ) -> Result<()>;
    async fn search_sessions(
        &self,
        user_id: Option<&str>,
        filters: &[crate::domain::model::SessionFilter],
        sort: &[crate::domain::model::SessionSort],
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<SessionSummary>, usize)>;

    /// 标记消息为已读（更新 last_read_msg_seq）
    async fn mark_as_read(
        &self,
        user_id: &str,
        session_id: &str,
        seq: i64,
    ) -> Result<()>;

    /// 获取未读数（基于 last_message_seq - last_read_msg_seq）
    async fn get_unread_count(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> Result<i32>;
}

/// Presence 仓储接口（需要作为 trait 对象使用，保留 async-trait）
#[async_trait]
pub trait PresenceRepository: Send + Sync {
    async fn list_devices(&self, user_id: &str) -> Result<Vec<DevicePresence>>;
    async fn update_presence(&self, update: PresenceUpdate) -> Result<()>;
}


#[async_trait]
pub trait MessageProvider: Send + Sync {
    async fn sync_messages(
        &self,
        session_id: &str,
        since_ts: i64,
        cursor: Option<&str>,
        limit: i32,
    ) -> Result<MessageSyncResult>;

    async fn recent_messages(
        &self,
        session_ids: &[String],
        limit_per_session: i32,
        client_cursor: &HashMap<String, i64>,
    ) -> Result<Vec<Message>>;

    /// 基于 seq 的消息同步（可选，用于优化性能）
    /// 
    /// # 参数
    /// * `session_id` - 会话ID
    /// * `after_seq` - 起始 seq（不包含）
    /// * `before_seq` - 结束 seq（可选，不包含）
    /// * `limit` - 返回消息数量限制
    /// 
    /// # 返回
    /// * `Ok(MessageSyncResult)` - 消息同步结果，包含消息列表和游标
    /// 
    /// # 注意
    /// 如果实现不支持基于 seq 的同步，可以返回 `Err` 或降级到基于时间戳的同步
    async fn sync_messages_by_seq(
        &self,
        session_id: &str,
        after_seq: i64,
        before_seq: Option<i64>,
        limit: i32,
    ) -> Result<MessageSyncResult> {
        // 默认实现：返回错误，提示使用基于时间戳的同步
        Err(anyhow::anyhow!("sync_messages_by_seq not implemented, use sync_messages instead"))
    }
}
