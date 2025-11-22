use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use prost::Message;
use redis::AsyncCommands;
use redis::aio::ConnectionManager;
use serde::Serialize;

use crate::domain::model::MessageSubmission;
use crate::domain::repository::WalRepository;
use crate::config::MessageOrchestratorConfig;

#[derive(Serialize)]
struct WalEntrySnapshot {
    message_id: String,
    encoded: String,
    persisted: bool,
}

pub struct RedisWalRepository {
    client: Arc<redis::Client>,
    config: Arc<MessageOrchestratorConfig>,
}

impl RedisWalRepository {
    pub fn new(client: Arc<redis::Client>, config: Arc<MessageOrchestratorConfig>) -> Self {
        Self { client, config }
    }

    async fn connection(&self) -> Result<ConnectionManager> {
        let manager = self
            .client
            .get_connection_manager()
            .await
            .map_err(anyhow::Error::new)?;
        Ok(manager)
    }
}

#[async_trait]
impl WalRepository for RedisWalRepository {
    async fn append(&self, submission: &MessageSubmission) -> Result<()> {
        let wal_key = match &self.config.wal_hash_key {
            Some(key) => key.as_str(),
            None => return Ok(()),
        };

        let mut conn = self.connection().await?;

        let encoded_payload = BASE64.encode(submission.kafka_payload.clone().encode_to_vec());
        let entry = WalEntrySnapshot {
            message_id: submission.message_id.clone(),
            encoded: encoded_payload,
            persisted: false,
        };

        let payload = serde_json::to_string(&entry)?;
        conn.hset::<_, _, _, ()>(
            wal_key,
            &submission.message_id,
            payload,
        )
        .await?;

        if self.config.wal_ttl_seconds > 0 {
            let _: () = conn
                .expire(wal_key, self.config.wal_ttl_seconds as i64)
                .await?;
        }

        Ok(())
    }
}
