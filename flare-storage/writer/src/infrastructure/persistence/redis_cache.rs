use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use redis::{AsyncCommands, aio::ConnectionManager};
use std::convert::TryInto;

use crate::domain::repositories::HotCacheRepository;
use crate::infrastructure::config::StorageWriterConfig;

pub struct RedisHotCacheRepository {
    client: Arc<redis::Client>,
    ttl_seconds: u64,
}

impl RedisHotCacheRepository {
    pub fn new(client: Arc<redis::Client>, config: &StorageWriterConfig) -> Self {
        Self {
            client,
            ttl_seconds: config.redis_hot_ttl_seconds,
        }
    }
}

#[async_trait]
impl HotCacheRepository for RedisHotCacheRepository {
    async fn store_hot(&self, stored: &flare_storage_model::StoredMessage) -> Result<()> {
        let mut conn = ConnectionManager::new(self.client.as_ref().clone()).await?;

        let message_key = format!(
            "cache:msg:{}:{}",
            stored.envelope.session_id, stored.envelope.message_id
        );
        let index_key = format!("cache:session:{}:index", stored.envelope.session_id);

        let json = serde_json::to_string(stored)?;
        let _: () = conn.set(&message_key, json).await?;
        if self.ttl_seconds > 0 {
            let ttl: i64 = self.ttl_seconds.try_into()?;
            let _: () = conn.expire(&message_key, ttl).await?;
        }

        let score = stored.timeline.ingestion_ts as f64;
        let _: () = conn
            .zadd(index_key.clone(), stored.envelope.message_id.clone(), score)
            .await?;
        if self.ttl_seconds > 0 {
            let ttl: i64 = self.ttl_seconds.try_into()?;
            let _: () = conn.expire(index_key, ttl).await?;
        }

        Ok(())
    }
}
