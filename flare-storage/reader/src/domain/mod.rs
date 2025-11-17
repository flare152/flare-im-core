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

    async fn count_messages(
        &self,
        session_id: &str,
        user_id: Option<&str>,
        start_time: Option<DateTime<Utc>>,
        end_time: Option<DateTime<Utc>>,
    ) -> Result<i64, Box<dyn std::error::Error>>;

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

    async fn search_messages(
        &self,
        filters: &[flare_proto::common::FilterExpression],
        start_time: Option<DateTime<Utc>>,
        end_time: Option<DateTime<Utc>>,
        limit: i32,
    ) -> Result<Vec<Message>, Box<dyn std::error::Error>>;

    async fn update_message_attributes(
        &self,
        message_id: &str,
        attributes: std::collections::HashMap<String, String>,
        tags: Vec<String>,
    ) -> Result<(), Box<dyn std::error::Error>>;

    async fn list_all_tags(
        &self,
    ) -> Result<Vec<String>, Box<dyn std::error::Error>>;
}

pub struct MessageUpdate {
    pub is_recalled: Option<bool>,
    pub recalled_at: Option<Timestamp>,
    // recall_reason字段在proto中不存在，已移除
    // 如果需要记录撤回原因，可以存储在extra字段中
    pub visibility: Option<HashMap<String, VisibilityStatus>>,
    pub read_by: Option<Vec<MessageReadRecord>>,
    pub operations: Option<Vec<MessageOperation>>,
    pub attributes: Option<HashMap<String, String>>,
    pub tags: Option<Vec<String>>,
}

// 注意：会话管理不属于 StorageService，已移除
// 会话管理应该由 SessionService 处理
// #[async_trait::async_trait]
// pub trait SessionStorage: Send + Sync {
//     ...
// }
// 
// pub struct SessionUpdate {
//     ...
// }

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
