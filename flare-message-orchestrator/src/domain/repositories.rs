use anyhow::Result;
use async_trait::async_trait;
use flare_proto::storage::StoreMessageRequest;

use super::message_submission::MessageSubmission;

#[async_trait]
pub trait MessageEventPublisher {
    async fn publish(&self, payload: StoreMessageRequest) -> Result<()>;
}

#[async_trait]
pub trait WalRepository {
    async fn append(&self, submission: &MessageSubmission) -> Result<()>;
}
