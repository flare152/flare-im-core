use std::sync::Arc;

use async_trait::async_trait;
use flare_server_core::error::Result;
use tracing::info;

use crate::domain::model::PushDispatchTask;

pub struct NoopOnlinePushSender;

#[async_trait]
impl crate::domain::repository::OnlinePushSender for NoopOnlinePushSender {
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
