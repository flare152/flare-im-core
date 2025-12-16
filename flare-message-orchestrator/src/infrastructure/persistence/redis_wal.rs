use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Result;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use prost::Message;
use redis::AsyncCommands;
use redis::aio::ConnectionManager;
use serde::Serialize;

use crate::config::MessageOrchestratorConfig;
use crate::domain::model::MessageSubmission;
use crate::domain::repository::WalRepository;

#[derive(Serialize)]
struct WalEntrySnapshot {
    message_id: String,
    encoded: String,
    persisted: bool,
}

#[derive(Debug)]
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

impl WalRepository for RedisWalRepository {
    fn append<'a>(
        &'a self,
        submission: &'a MessageSubmission,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        let _self = self; // 保持对 self 的引用
        let _submission = submission; // 保持对 submission 的引用
        Box::pin(async move {
            let wal_key = match &_self.config.wal_hash_key {
                Some(key) => key.as_str(),
                None => return Ok(()),
            };

            let mut conn = _self.connection().await?;

            let encoded_payload = BASE64.encode(_submission.kafka_payload.clone().encode_to_vec());
            let entry = WalEntrySnapshot {
                message_id: _submission.message_id.clone(),
                encoded: encoded_payload,
                persisted: false,
            };

            let payload = serde_json::to_string(&entry)?;
            conn.hset::<_, _, _, ()>(wal_key, &_submission.message_id, payload)
                .await?;

            if _self.config.wal_ttl_seconds > 0 {
                let _: () = conn
                    .expire(wal_key, _self.config.wal_ttl_seconds as i64)
                    .await?;
            }

            Ok(())
        })
    }
}
