use async_trait::async_trait;
use flare_server_core::error::Result;

use crate::domain::models::PushDispatchTask;

#[async_trait]
pub trait OnlineStatusRepository: Send + Sync {
    async fn is_online(&self, user_id: &str) -> Result<bool>;
}

#[async_trait]
pub trait PushTaskPublisher: Send + Sync {
    async fn publish(&self, task: &PushDispatchTask) -> Result<()>;
}
