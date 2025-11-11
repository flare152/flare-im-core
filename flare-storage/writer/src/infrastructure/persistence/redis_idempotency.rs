use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use redis::{AsyncCommands, aio::ConnectionManager};
use std::convert::TryInto;

use crate::domain::repositories::MessageIdempotencyRepository;
use crate::infrastructure::config::StorageWriterConfig;

pub struct RedisIdempotencyRepository {
    client: Arc<redis::Client>,
    ttl_seconds: u64,
}

impl RedisIdempotencyRepository {
    pub fn new(client: Arc<redis::Client>, config: &StorageWriterConfig) -> Self {
        Self {
            client,
            ttl_seconds: config.redis_idempotency_ttl_seconds,
        }
    }
}

#[async_trait]
impl MessageIdempotencyRepository for RedisIdempotencyRepository {
    async fn is_new(&self, message_id: &str) -> Result<bool> {
        let mut conn = ConnectionManager::new(self.client.as_ref().clone()).await?;

        let key = format!("storage:idempotency:{}", message_id);
        let is_new: bool = conn.set_nx(&key, 1).await?;
        if is_new && self.ttl_seconds > 0 {
            let ttl: i64 = self.ttl_seconds.try_into()?;
            let _: () = conn.expire(&key, ttl).await?;
        }

        Ok(is_new)
    }
}
