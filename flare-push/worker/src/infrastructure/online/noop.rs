use std::sync::Arc;

use async_trait::async_trait;
use flare_server_core::error::Result;
use tracing::info;

use crate::domain::models::PushDispatchTask;
use crate::domain::repositories::OnlinePushSender;

pub struct NoopOnlinePushSender;

#[async_trait]
impl OnlinePushSender for NoopOnlinePushSender {
    async fn send(&self, task: &PushDispatchTask) -> Result<()> {
        info!(user_id = %task.user_id, "noop online push sender invoked");
        Ok(())
    }
}

impl NoopOnlinePushSender {
    pub fn shared() -> Arc<Self> {
        Arc::new(Self)
    }
}
