use std::sync::Arc;

use async_trait::async_trait;
use flare_server_core::error::Result;
use tracing::info;

use crate::domain::models::PushDispatchTask;
use crate::domain::repositories::OfflinePushSender;

pub struct NoopOfflinePushSender;

#[async_trait]
impl OfflinePushSender for NoopOfflinePushSender {
    async fn send(&self, task: &PushDispatchTask) -> Result<()> {
        info!(user_id = %task.user_id, "noop offline push sender invoked");
        Ok(())
    }
}

impl NoopOfflinePushSender {
    pub fn shared() -> Arc<Self> {
        Arc::new(Self)
    }
}
