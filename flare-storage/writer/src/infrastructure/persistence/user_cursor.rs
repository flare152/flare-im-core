use std::sync::Arc;
use async_trait::async_trait;

use anyhow::Result;
use redis::{AsyncCommands, aio::ConnectionManager};

use crate::domain::repository::UserSyncCursorRepository;

pub struct RedisUserCursorRepository {
    client: Arc<redis::Client>,
}

impl RedisUserCursorRepository {
    pub fn new(client: Arc<redis::Client>) -> Self {
        Self { client }
    }

    async fn connection(&self) -> Result<ConnectionManager> {
        Ok(ConnectionManager::new(self.client.as_ref().clone()).await?)
    }
}


#[async_trait]
impl UserSyncCursorRepository for RedisUserCursorRepository {
    async fn advance_cursor(&self, session_id: &str, user_id: &str, message_ts: i64) -> Result<()> {
        let mut conn = self.connection().await?;
        let cursor_key = format!("storage:user:cursor:{}", user_id);
        let _: () = conn
            .hset(cursor_key, session_id, message_ts.to_string())
            .await?;
        Ok(())
    }
}
