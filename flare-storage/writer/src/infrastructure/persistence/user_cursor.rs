use async_trait::async_trait;
use std::sync::Arc;

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
    async fn advance_cursor(&self, ctx: &flare_server_core::context::Context, conversation_id: &str, message_ts: i64) -> Result<()> {
        let user_id = ctx.user_id().ok_or_else(|| anyhow::anyhow!("user_id is required in context"))?;
        let mut conn = self.connection().await?;
        let cursor_key = format!("storage:user:cursor:{}", user_id);
        let _: () = conn
            .hset(cursor_key, conversation_id, message_ts.to_string())
            .await?;
        Ok(())
    }
}
