use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use redis::{AsyncCommands, aio::ConnectionManager};

use crate::domain::repository::WalCleanupRepository;

pub struct RedisWalCleanupRepository {
    client: Arc<redis::Client>,
    wal_key: String,
}

impl RedisWalCleanupRepository {
    pub fn new(client: Arc<redis::Client>, wal_key: String) -> Self {
        Self { client, wal_key }
    }
}

#[async_trait]
impl WalCleanupRepository for RedisWalCleanupRepository {
    async fn remove(&self, message_id: &str) -> Result<()> {
        let mut conn = ConnectionManager::new(self.client.as_ref().clone()).await?;
        let _: () = conn.hdel(&self.wal_key, message_id).await?;
        Ok(())
    }
}
