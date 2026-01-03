use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Result;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use prost::Message;
use redis::AsyncCommands;
use redis::aio::ConnectionManager;
use serde::{Serialize, Deserialize};

use crate::config::MessageOrchestratorConfig;
use crate::domain::model::MessageSubmission;
use crate::domain::repository::WalRepository;

#[derive(Serialize, Deserialize)]
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
                None => {
                    tracing::debug!(
                        message_id = %_submission.message_id,
                        "WAL not configured (wal_hash_key is None), skipping WAL write"
                    );
                    return Ok(());
                }
            };

            let mut conn = _self.connection().await?;

            // 使用 message.server_id 作为 WAL key（确保与查询时一致）
            // 注意：submission.message_id 应该等于 submission.message.server_id，但为了安全起见，直接使用 message.server_id
            let wal_message_id = _submission.message.server_id.clone();
            
            let encoded_payload = BASE64.encode(_submission.kafka_payload.clone().encode_to_vec());
            let entry = WalEntrySnapshot {
                message_id: wal_message_id.clone(),
                encoded: encoded_payload,
                persisted: false,
            };

            let payload = serde_json::to_string(&entry)?;
            conn.hset::<_, _, _, ()>(wal_key, &wal_message_id, payload)
                .await?;

            if _self.config.wal_ttl_seconds > 0 {
                let _: () = conn
                    .expire(wal_key, _self.config.wal_ttl_seconds as i64)
                    .await?;
            }

            tracing::debug!(
                message_id = %wal_message_id,
                submission_message_id = %_submission.message_id,
                wal_key = %wal_key,
                ttl_seconds = %_self.config.wal_ttl_seconds,
                "✅ WAL entry written successfully"
            );

            Ok(())
        })
    }

    fn find_by_message_id<'a>(
        &'a self,
        message_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<flare_proto::common::Message>>> + Send + 'a>> {
        let _self = self;
        let _message_id = message_id.to_string();
        Box::pin(async move {
            let wal_key = match &_self.config.wal_hash_key {
                Some(key) => key.as_str(),
                None => {
                    tracing::debug!(
                        message_id = %_message_id,
                        "WAL not configured (wal_hash_key is None), cannot query WAL"
                    );
                    return Ok(None);
                }
            };

            tracing::debug!(
                message_id = %_message_id,
                wal_key = %wal_key,
                "🔍 Querying WAL for message"
            );

            let mut conn = _self.connection().await?;

            // 从 Redis Hash 中查询
            let entry_json: Option<String> = conn.hget(wal_key, &_message_id).await?;
            
            if let Some(json_str) = entry_json {
                tracing::debug!(
                    message_id = %_message_id,
                    "✅ Found WAL entry, decoding..."
                );
                // 反序列化 WalEntrySnapshot
                let entry: WalEntrySnapshot = serde_json::from_str(&json_str)
                    .map_err(|e| anyhow::anyhow!("Failed to deserialize WAL entry: {}", e))?;
                
                // 解码 base64 编码的 payload
                let payload_bytes = BASE64.decode(&entry.encoded)
                    .map_err(|e| anyhow::anyhow!("Failed to decode base64 payload from WAL: {}", e))?;
                
                // 反序列化为 StoreMessageRequest
                let request = flare_proto::storage::StoreMessageRequest::decode(&payload_bytes[..])
                    .map_err(|e| anyhow::anyhow!("Failed to decode StoreMessageRequest from WAL: {}", e))?;
                
                // 提取 Message
                if let Some(message) = request.message {
                    tracing::info!(
                        message_id = %_message_id,
                        sender_id = %message.sender_id,
                        "✅ Successfully retrieved message from WAL"
                    );
                    Ok(Some(message))
                } else {
                    tracing::warn!(
                        message_id = %_message_id,
                        "WAL entry found but message field is None"
                    );
                    Ok(None)
                }
            } else {
                tracing::debug!(
                    message_id = %_message_id,
                    wal_key = %wal_key,
                    "WAL entry not found in Redis"
                );
                Ok(None)
            }
        })
    }
}
