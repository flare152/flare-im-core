use anyhow::Result;
use async_trait::async_trait;
use flare_proto::storage::Message;

use super::events::AckEvent;
use super::message_persistence::MediaAttachmentMetadata;

#[async_trait]
pub trait MessageIdempotencyRepository: Send + Sync {
    async fn is_new(&self, message_id: &str) -> Result<bool>;
}

#[async_trait]
pub trait HotCacheRepository: Send + Sync {
    async fn store_hot(&self, message: &Message) -> Result<()>;
}

#[async_trait]
pub trait RealtimeStoreRepository: Send + Sync {
    async fn store_realtime(&self, message: &Message) -> Result<()>;
}

#[async_trait]
pub trait ArchiveStoreRepository: Send + Sync {
    async fn store_archive(&self, message: &Message) -> Result<()>;
}

#[async_trait]
pub trait WalCleanupRepository: Send + Sync {
    async fn remove(&self, message_id: &str) -> Result<()>;
}

#[async_trait]
pub trait SessionStateRepository: Send + Sync {
    async fn apply_message(&self, message: &Message) -> Result<()>;
}

#[async_trait]
pub trait UserSyncCursorRepository: Send + Sync {
    async fn advance_cursor(&self, session_id: &str, user_id: &str, message_ts: i64) -> Result<()>;
}

#[async_trait]
pub trait AckPublisher: Send + Sync {
    async fn publish(&self, event: AckEvent<'_>) -> Result<()>;
}

#[async_trait]
pub trait MediaAttachmentVerifier: Send + Sync {
    async fn fetch_metadata(&self, file_ids: &[String]) -> Result<Vec<MediaAttachmentMetadata>>;
}
