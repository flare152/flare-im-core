use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use crate::domain::message_submission::MessageSubmission;
use crate::domain::repositories::WalRepository;

#[derive(Default)]
pub struct NoopWalRepository;

#[async_trait]
impl WalRepository for NoopWalRepository {
    async fn append(&self, _submission: &MessageSubmission) -> Result<()> {
        Ok(())
    }
}

impl NoopWalRepository {
    pub fn shared() -> Arc<dyn WalRepository + Send + Sync> {
        Arc::new(Self::default())
    }
}
