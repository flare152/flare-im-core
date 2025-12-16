//! 消息状态仓储实现
//!
//! 负责存储和查询用户对消息的私有行为（已读、删除、阅后即焚）

use anyhow::{Context, Result};
use sqlx::{PgPool, Row};
use std::sync::Arc;
use tracing::{debug, instrument};

use crate::domain::repository::MessageStateRepository;
use async_trait::async_trait;

/// PostgreSQL 消息状态仓储实现
pub struct PostgresMessageStateRepository {
    pool: Arc<PgPool>,
}

impl PostgresMessageStateRepository {
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl MessageStateRepository for PostgresMessageStateRepository {
    #[instrument(skip(self), fields(message_id = %message_id, user_id = %user_id))]
    async fn mark_as_read(&self, message_id: &str, user_id: &str) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO message_state (message_id, user_id, is_read, read_at, updated_at)
            VALUES ($1, $2, TRUE, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
            ON CONFLICT (message_id, user_id)
            DO UPDATE SET
                is_read = TRUE,
                read_at = CURRENT_TIMESTAMP,
                updated_at = CURRENT_TIMESTAMP
            "#,
        )
        .bind(message_id)
        .bind(user_id)
        .execute(self.pool.as_ref())
        .await
        .context("Failed to mark message as read")?;

        debug!(
            message_id = %message_id,
            user_id = %user_id,
            "Marked message as read"
        );

        Ok(())
    }

    #[instrument(skip(self), fields(message_id = %message_id, user_id = %user_id))]
    async fn mark_as_deleted(&self, message_id: &str, user_id: &str) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO message_state (message_id, user_id, is_deleted, deleted_at, updated_at)
            VALUES ($1, $2, TRUE, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
            ON CONFLICT (message_id, user_id)
            DO UPDATE SET
                is_deleted = TRUE,
                deleted_at = CURRENT_TIMESTAMP,
                updated_at = CURRENT_TIMESTAMP
            "#,
        )
        .bind(message_id)
        .bind(user_id)
        .execute(self.pool.as_ref())
        .await
        .context("Failed to mark message as deleted")?;

        debug!(
            message_id = %message_id,
            user_id = %user_id,
            "Marked message as deleted"
        );

        Ok(())
    }

    #[instrument(skip(self), fields(message_id = %message_id, user_id = %user_id))]
    async fn mark_as_burned(&self, message_id: &str, user_id: &str) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO message_state (message_id, user_id, burn_after_read, burned_at, updated_at)
            VALUES ($1, $2, TRUE, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
            ON CONFLICT (message_id, user_id)
            DO UPDATE SET
                burn_after_read = TRUE,
                burned_at = CURRENT_TIMESTAMP,
                updated_at = CURRENT_TIMESTAMP
            "#,
        )
        .bind(message_id)
        .bind(user_id)
        .execute(self.pool.as_ref())
        .await
        .context("Failed to mark message as burned")?;

        debug!(
            message_id = %message_id,
            user_id = %user_id,
            "Marked message as burned"
        );

        Ok(())
    }

    #[instrument(skip(self), fields(message_id = %message_id, user_id = %user_id))]
    async fn is_read(&self, message_id: &str, user_id: &str) -> Result<bool> {
        let row = sqlx::query(
            r#"
            SELECT is_read
            FROM message_state
            WHERE message_id = $1 AND user_id = $2
            "#,
        )
        .bind(message_id)
        .bind(user_id)
        .fetch_optional(self.pool.as_ref())
        .await
        .context("Failed to check if message is read")?;

        Ok(row.map(|r| r.get("is_read")).unwrap_or(false))
    }

    #[instrument(skip(self), fields(message_id = %message_id, user_id = %user_id))]
    async fn is_deleted(&self, message_id: &str, user_id: &str) -> Result<bool> {
        let row = sqlx::query(
            r#"
            SELECT is_deleted
            FROM message_state
            WHERE message_id = $1 AND user_id = $2
            "#,
        )
        .bind(message_id)
        .bind(user_id)
        .fetch_optional(self.pool.as_ref())
        .await
        .context("Failed to check if message is deleted")?;

        Ok(row.map(|r| r.get("is_deleted")).unwrap_or(false))
    }

    #[instrument(skip(self), fields(message_id = %message_id, user_id = %user_id))]
    async fn is_burned(&self, message_id: &str, user_id: &str) -> Result<bool> {
        let row = sqlx::query(
            r#"
            SELECT burned_at IS NOT NULL as is_burned
            FROM message_state
            WHERE message_id = $1 AND user_id = $2
            "#,
        )
        .bind(message_id)
        .bind(user_id)
        .fetch_optional(self.pool.as_ref())
        .await
        .context("Failed to check if message is burned")?;

        Ok(row.map(|r| r.get::<bool, _>("is_burned")).unwrap_or(false))
    }

    #[instrument(skip(self), fields(user_id = %user_id, message_ids = message_ids.len()))]
    async fn batch_check_deleted(
        &self,
        user_id: &str,
        message_ids: &[String],
    ) -> Result<Vec<String>> {
        if message_ids.is_empty() {
            return Ok(Vec::new());
        }

        // 使用 PostgreSQL 数组类型
        let rows = sqlx::query(
            r#"
            SELECT message_id
            FROM message_state
            WHERE user_id = $1 AND message_id = ANY($2::text[]) AND is_deleted = TRUE
            "#,
        )
        .bind(user_id)
        .bind(message_ids)
        .fetch_all(self.pool.as_ref())
        .await
        .context("Failed to batch check deleted messages")?;

        Ok(rows
            .into_iter()
            .map(|r| r.get::<String, _>("message_id"))
            .collect())
    }
}
