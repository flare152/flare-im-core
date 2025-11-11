use std::collections::HashMap;

use async_trait::async_trait;
use flare_server_core::error::Result;

use crate::domain::entities::{OnlineStatusRecord, SessionRecord};

#[async_trait]
pub trait SessionRepository: Send + Sync {
    async fn save_session(&self, record: &SessionRecord) -> Result<()>;
    async fn remove_session(&self, session_id: &str, user_id: &str) -> Result<()>;
    async fn touch_session(&self, user_id: &str) -> Result<()>;
    async fn fetch_statuses(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, OnlineStatusRecord>>;
}
