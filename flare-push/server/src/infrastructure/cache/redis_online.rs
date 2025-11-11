use std::sync::Arc;

use async_trait::async_trait;
use flare_server_core::error::{ErrorBuilder, ErrorCode, Result};
use redis::{AsyncCommands, aio::ConnectionManager};
use serde_json::json;

use crate::domain::repositories::OnlineStatusRepository;

pub struct RedisOnlineStatusRepository {
    client: Arc<redis::Client>,
    ttl_seconds: u64,
}

impl RedisOnlineStatusRepository {
    pub fn new(client: Arc<redis::Client>, ttl_seconds: u64) -> Self {
        Self {
            client,
            ttl_seconds,
        }
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

    fn key(user_id: &str) -> String {
        format!("push:online:{}", user_id)
    }
}

#[async_trait]
impl OnlineStatusRepository for RedisOnlineStatusRepository {
    async fn is_online(&self, user_id: &str) -> Result<bool> {
        let mut conn = self.connection().await?;

        let key = Self::key(user_id);
        let exists: bool = conn.exists(&key).await.map_err(|err| {
            ErrorBuilder::new(ErrorCode::DatabaseError, "failed to query online status")
                .details(err.to_string())
                .build_error()
        })?;

        if exists {
            let _: () = conn
                .set_ex(&key, json!(true).to_string(), self.ttl_seconds)
                .await
                .map_err(|err| {
                    ErrorBuilder::new(ErrorCode::DatabaseError, "failed to refresh online ttl")
                        .details(err.to_string())
                        .build_error()
                })?;
        }

        Ok(exists)
    }
}
