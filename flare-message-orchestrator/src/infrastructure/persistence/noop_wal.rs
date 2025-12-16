use std::future::Future;
use std::pin::Pin;

use anyhow::Result;

use crate::domain::model::MessageSubmission;
use crate::domain::repository::WalRepository;

#[derive(Debug, Default)]
pub struct NoopWalRepository;

impl WalRepository for NoopWalRepository {
    fn append<'a>(
        &'a self,
        _submission: &'a MessageSubmission,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move { Ok(()) })
    }
}

// shared() 方法已移除，现在使用 WalRepositoryItem::Noop 代替
