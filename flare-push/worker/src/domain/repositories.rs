use async_trait::async_trait;
use flare_server_core::error::Result;

use super::models::PushDispatchTask;

#[async_trait]
pub trait OnlinePushSender: Send + Sync {
    async fn send(&self, task: &PushDispatchTask) -> Result<()>;
}

#[async_trait]
pub trait OfflinePushSender: Send + Sync {
    async fn send(&self, task: &PushDispatchTask) -> Result<()>;
}
