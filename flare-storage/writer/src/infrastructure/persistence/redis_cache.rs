use std::sync::Arc;
use async_trait::async_trait;

use anyhow::Result;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use prost::Message as _;
use redis::{AsyncCommands, aio::ConnectionManager};
use std::convert::TryInto;

use crate::config::StorageWriterConfig;
use crate::domain::repository::HotCacheRepository;

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
    async fn store_hot(&self, message: &flare_proto::common::Message) -> Result<()> {
        let mut conn = ConnectionManager::new(self.client.as_ref().clone()).await?;

        let message_key = format!("cache:msg:{}:{}", message.session_id, message.id);
        let index_key = format!("cache:session:{}:index", message.session_id);

        // 将 Message 编码为 protobuf bytes，然后 base64 编码存储
        let mut buf = Vec::new();
        message.encode(&mut buf)?;
        let encoded = BASE64.encode(&buf);
        let _: () = conn.set(&message_key, encoded).await?;
        if self.ttl_seconds > 0 {
            let ttl: i64 = self.ttl_seconds.try_into()?;
            let _: () = conn.expire(&message_key, ttl).await?;
        }

        // 从 extra 中提取 ingestion_ts，如果没有则使用当前时间
        let ingestion_ts = flare_im_core::utils::extract_timeline_from_extra(
            &message.extra,
            flare_im_core::utils::current_millis(),
        )
        .ingestion_ts;
        let score = ingestion_ts as f64;
        let _: () = conn
            .zadd(index_key.clone(), message.id.clone(), score)
            .await?;
        if self.ttl_seconds > 0 {
            let ttl: i64 = self.ttl_seconds.try_into()?;
            let _: () = conn.expire(index_key, ttl).await?;
        }

        Ok(())
    }
}
