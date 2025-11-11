use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use flare_server_core::error::{ErrorBuilder, ErrorCode, Result};
use redis::{AsyncCommands, aio::ConnectionManager};
use serde_json::json;

use crate::config::OnlineConfig;
use crate::domain::entities::{OnlineStatusRecord, SessionRecord};
use crate::domain::repositories::SessionRepository;

const SESSION_KEY_PREFIX: &str = "session";

pub struct RedisSessionRepository {
    client: Arc<redis::Client>,
    config: Arc<OnlineConfig>,
}

impl RedisSessionRepository {
    pub fn new(client: Arc<redis::Client>, config: Arc<OnlineConfig>) -> Self {
        Self { client, config }
    }

    fn session_key(&self, user_id: &str) -> String {
        format!("{}:{}", SESSION_KEY_PREFIX, user_id)
    }

    async fn connection(&self) -> Result<ConnectionManager> {
        ConnectionManager::new(self.client.as_ref().clone())
            .await
            .map_err(|err| {
                ErrorBuilder::new(
                    ErrorCode::ServiceUnavailable,
                    "failed to open redis connection",
                )
                .details(err.to_string())
                .build_error()
            })
    }

    fn to_timestamp(seconds: i64) -> Option<DateTime<Utc>> {
        Utc.timestamp_opt(seconds, 0).single()
    }
}

#[async_trait]
impl SessionRepository for RedisSessionRepository {
    async fn save_session(&self, record: &SessionRecord) -> Result<()> {
        let mut conn = self.connection().await?;
        let key = self.session_key(&record.user_id);
        let value = json!({
            "session_id": record.session_id,
            "gateway_id": record.gateway_id,
            "server_id": record.server_id,
            "device_id": record.device_id,
            "last_seen": record.last_seen.timestamp(),
        });
        let _: () = conn.set(&key, value.to_string()).await.map_err(|err| {
            ErrorBuilder::new(ErrorCode::DatabaseError, "failed to store session")
                .details(err.to_string())
                .build_error()
        })?;
        let _: bool = conn
            .expire(&key, self.config.redis_ttl_seconds as i64)
            .await
            .map_err(|err| {
                ErrorBuilder::new(ErrorCode::DatabaseError, "failed to set session ttl")
                    .details(err.to_string())
                    .build_error()
            })?;
        Ok(())
    }

    async fn remove_session(&self, session_id: &str, user_id: &str) -> Result<()> {
        let mut conn = self.connection().await?;
        let key = self.session_key(user_id);
        let _: usize = conn.del(&key).await.map_err(|err| {
            ErrorBuilder::new(ErrorCode::DatabaseError, "failed to delete session")
                .details(err.to_string())
                .build_error()
        })?;
        tracing::info!(%session_id, %user_id, "session removed from redis");
        Ok(())
    }

    async fn touch_session(&self, user_id: &str) -> Result<()> {
        let mut conn = self.connection().await?;
        let key = self.session_key(user_id);
        let _: bool = conn
            .expire(&key, self.config.redis_ttl_seconds as i64)
            .await
            .map_err(|err| {
                ErrorBuilder::new(ErrorCode::DatabaseError, "failed to refresh session ttl")
                    .details(err.to_string())
                    .build_error()
            })?;
        Ok(())
    }

    async fn fetch_statuses(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, OnlineStatusRecord>> {
        let mut conn = self.connection().await?;
        let mut result = HashMap::new();
        for user_id in user_ids {
            let key = self.session_key(user_id);
            let value: Option<String> = conn.get(&key).await.map_err(|err| {
                ErrorBuilder::new(ErrorCode::DatabaseError, "failed to read session")
                    .details(err.to_string())
                    .build_error()
            })?;
            if let Some(payload) = value {
                let json: serde_json::Value = serde_json::from_str(&payload).map_err(|err| {
                    ErrorBuilder::new(
                        ErrorCode::DeserializationError,
                        "failed to decode session json",
                    )
                    .details(err.to_string())
                    .build_error()
                })?;
                let last_seen = json
                    .get("last_seen")
                    .and_then(|v| v.as_i64())
                    .and_then(Self::to_timestamp);
                result.insert(
                    user_id.clone(),
                    OnlineStatusRecord {
                        online: true,
                        server_id: json
                            .get("server_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        gateway_id: json
                            .get("gateway_id")
                            .and_then(|v| v.as_str())
                            .map(|v| v.to_string()),
                        cluster_id: None,
                        last_seen,
                    },
                );
            }
        }

        Ok(result)
    }
}
