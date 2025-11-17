use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;
use flare_proto::storage::Message;

use super::models::{
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
        filters: &[crate::domain::models::SessionFilter],
        sort: &[crate::domain::models::SessionSort],
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<SessionSummary>, usize)>;
}

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
}
