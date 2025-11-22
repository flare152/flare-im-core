//! 仓储接口定义（Port）

use std::collections::HashMap;
use anyhow::Result;
use chrono::{DateTime, Utc};
use flare_proto::common::{Message, VisibilityStatus};
use crate::domain::model::MessageUpdate;

// Rust 2024: trait 中直接使用 async fn
// 注意：对于 trait 对象（dyn Trait），仍需要使用 async_trait
#[async_trait::async_trait]
pub trait MessageStorage: Send + Sync {
    async fn store_message(
        &self,
        message: &Message,
        session_id: &str,
    ) -> Result<()>;

    async fn query_messages(
        &self,
        session_id: &str,
        user_id: Option<&str>,
        start_time: Option<DateTime<Utc>>,
        end_time: Option<DateTime<Utc>>,
        limit: i32,
    ) -> Result<Vec<Message>>;

    async fn count_messages(
        &self,
        session_id: &str,
        user_id: Option<&str>,
        start_time: Option<DateTime<Utc>>,
        end_time: Option<DateTime<Utc>>,
    ) -> Result<i64>;

    async fn get_message(
        &self,
        message_id: &str,
    ) -> Result<Option<Message>>;

    async fn update_message(
        &self,
        message_id: &str,
        updates: MessageUpdate,
    ) -> Result<()>;

    async fn batch_update_visibility(
        &self,
        message_ids: &[String],
        user_id: &str,
        visibility: VisibilityStatus,
    ) -> Result<usize>;

    async fn search_messages(
        &self,
        filters: &[flare_proto::common::FilterExpression],
        start_time: Option<DateTime<Utc>>,
        end_time: Option<DateTime<Utc>>,
        limit: i32,
    ) -> Result<Vec<Message>>;

    async fn update_message_attributes(
        &self,
        message_id: &str,
        attributes: HashMap<String, String>,
        tags: Vec<String>,
    ) -> Result<()>;

    async fn list_all_tags(
        &self,
    ) -> Result<Vec<String>>;
}

#[async_trait::async_trait]
pub trait VisibilityStorage: Send + Sync {
    async fn set_visibility(
        &self,
        message_id: &str,
        user_id: &str,
        session_id: &str,
        visibility: VisibilityStatus,
    ) -> Result<()>;

    async fn get_visibility(
        &self,
        message_id: &str,
        user_id: &str,
    ) -> Result<Option<VisibilityStatus>>;

    async fn batch_set_visibility(
        &self,
        message_ids: &[String],
        user_id: &str,
        session_id: &str,
        visibility: VisibilityStatus,
    ) -> Result<usize>;

    async fn query_visible_message_ids(
        &self,
        user_id: &str,
        session_id: &str,
        visibility_status: VisibilityStatus,
    ) -> Result<Vec<String>>;
}

