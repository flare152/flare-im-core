//! 读侧领域接口定义

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use flare_proto::storage::*;
use prost_types::Timestamp;

#[async_trait::async_trait]
pub trait MessageStorage: Send + Sync {
    async fn store_message(
        &self,
        message: &Message,
        session_id: &str,
    ) -> Result<(), Box<dyn std::error::Error>>;

    async fn query_messages(
        &self,
        session_id: &str,
        user_id: Option<&str>,
        start_time: Option<DateTime<Utc>>,
        end_time: Option<DateTime<Utc>>,
        limit: i32,
    ) -> Result<Vec<Message>, Box<dyn std::error::Error>>;

    async fn get_message(
        &self,
        message_id: &str,
    ) -> Result<Option<Message>, Box<dyn std::error::Error>>;

    async fn update_message(
        &self,
        message_id: &str,
        updates: MessageUpdate,
    ) -> Result<(), Box<dyn std::error::Error>>;

    async fn batch_update_visibility(
        &self,
        message_ids: &[String],
        user_id: &str,
        visibility: VisibilityStatus,
    ) -> Result<usize, Box<dyn std::error::Error>>;
}

pub struct MessageUpdate {
    pub is_recalled: Option<bool>,
    pub recalled_at: Option<Timestamp>,
    pub recall_reason: Option<String>,
    pub visibility: Option<HashMap<String, VisibilityStatus>>,
    pub read_by: Option<Vec<MessageReadRecord>>,
    pub operations: Option<Vec<MessageOperation>>,
}

#[async_trait::async_trait]
pub trait SessionStorage: Send + Sync {
    async fn create_session(&self, session: &SessionInfo)
    -> Result<(), Box<dyn std::error::Error>>;

    async fn get_session(
        &self,
        session_id: &str,
    ) -> Result<Option<SessionInfo>, Box<dyn std::error::Error>>;

    async fn update_session(
        &self,
        session_id: &str,
        updates: SessionUpdate,
    ) -> Result<(), Box<dyn std::error::Error>>;

    async fn query_user_sessions(
        &self,
        user_id: &str,
        business_type: Option<&str>,
        limit: i32,
        cursor: Option<&str>,
    ) -> Result<(Vec<SessionInfo>, Option<String>), Box<dyn std::error::Error>>;
}

pub struct SessionUpdate {
    pub last_message_id: Option<String>,
    pub last_message_time: Option<Timestamp>,
    pub unread_count: Option<HashMap<String, i32>>,
    pub status: Option<String>,
    pub metadata: Option<HashMap<String, String>>,
}

#[async_trait::async_trait]
pub trait VisibilityStorage: Send + Sync {
    async fn set_visibility(
        &self,
        message_id: &str,
        user_id: &str,
        session_id: &str,
        visibility: VisibilityStatus,
    ) -> Result<(), Box<dyn std::error::Error>>;

    async fn get_visibility(
        &self,
        message_id: &str,
        user_id: &str,
    ) -> Result<Option<VisibilityStatus>, Box<dyn std::error::Error>>;

    async fn batch_set_visibility(
        &self,
        message_ids: &[String],
        user_id: &str,
        session_id: &str,
        visibility: VisibilityStatus,
    ) -> Result<usize, Box<dyn std::error::Error>>;

    async fn query_visible_message_ids(
        &self,
        user_id: &str,
        session_id: &str,
        visibility_status: VisibilityStatus,
    ) -> Result<Vec<String>, Box<dyn std::error::Error>>;
}
