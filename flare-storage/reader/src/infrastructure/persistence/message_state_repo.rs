//! 消息状态仓储实现
//!
//! 负责存储和查询用户对消息的私有行为（已读、删除、阅后即焚）

use anyhow::{Context, Result};
use sqlx::{Pool, Postgres, Row};
use std::sync::Arc;
use tracing::{debug, instrument};

use crate::domain::repository::MessageStateRepository;
use async_trait::async_trait;

/// PostgreSQL 消息状态仓储实现
pub struct PostgresMessageStateRepository {
    pool: Arc<Pool<Postgres>>,
}

impl PostgresMessageStateRepository {
    pub fn new(pool: Arc<Pool<Postgres>>) -> Self {
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
            "Marked message as read in message_state table"
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
            "Marked message as deleted in message_state table"
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
            "Marked message as burned in message_state table"
        );

        Ok(())
    }

    #[instrument(skip(self), fields(user_id = %user_id, message_count = message_ids.len()))]
    async fn batch_mark_as_read(&self, user_id: &str, message_ids: &[String]) -> Result<()> {
        if message_ids.is_empty() {
            return Ok(());
        }

        // 使用 PostgreSQL 的批量插入/更新
        for message_id in message_ids {
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
            .context(format!("Failed to mark message {} as read", message_id))?;
        }

        debug!(
            user_id = %user_id,
            message_count = message_ids.len(),
            "Batch marked messages as read in message_state table"
        );

        Ok(())
    }

    #[instrument(skip(self), fields(user_id = %user_id, message_count = message_ids.len()))]
    async fn batch_mark_as_deleted(&self, user_id: &str, message_ids: &[String]) -> Result<()> {
        if message_ids.is_empty() {
            return Ok(());
        }

        // 使用 PostgreSQL 的批量插入/更新
        for message_id in message_ids {
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
            .context(format!("Failed to mark message {} as deleted", message_id))?;
        }

        debug!(
            user_id = %user_id,
            message_count = message_ids.len(),
            "Batch marked messages as deleted in message_state table"
        );

        Ok(())
    }
}
