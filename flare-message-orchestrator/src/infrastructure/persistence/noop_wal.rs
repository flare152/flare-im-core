use std::sync::Arc;

use anyhow::Result;

use crate::domain::model::MessageSubmission;
use crate::domain::repository::WalRepository;

#[derive(Debug, Default)]
pub struct NoopWalRepository;

impl WalRepository for NoopWalRepository {
    async fn append(&self, _submission: &MessageSubmission) -> Result<()> {
        Ok(())
    }
}

// shared() 方法已移除，现在使用 WalRepositoryItem::Noop 代替
