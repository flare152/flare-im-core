//! 会话仓储实现
//!
//! 负责更新会话和参与者信息（last_message_seq、unread_count等）

use anyhow::Result;
use async_trait::async_trait;
use sqlx::PgPool;
use std::sync::Arc;
use tracing::{debug, instrument};

use crate::domain::repository::SessionUpdateRepository;

/// PostgreSQL 会话仓储实现
pub struct PostgresSessionRepository {
    pool: Arc<PgPool>,
}

impl PostgresSessionRepository {
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SessionUpdateRepository for PostgresSessionRepository {
    #[instrument(skip(self), fields(session_id = %session_id, message_id = %message_id, seq))]
    async fn update_last_message(
        &self,
        session_id: &str,
        message_id: &str,
        seq: i64,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE sessions
            SET 
                last_message_id = $1,
                last_message_seq = $2,
                updated_at = CURRENT_TIMESTAMP
            WHERE session_id = $3
            "#,
        )
        .bind(message_id)
        .bind(seq)
        .bind(session_id)
        .execute(self.pool.as_ref())
        .await?;

        debug!(
            session_id = %session_id,
            message_id = %message_id,
            seq,
            "Updated session last_message"
        );

        Ok(())
    }

    #[instrument(skip(self), fields(session_id = %session_id))]
    async fn batch_update_unread_count(
        &self,
        session_id: &str,
        last_message_seq: i64,
        exclude_user_id: Option<&str>,
    ) -> Result<()> {
        // 批量更新所有参与者的未读数（排除发送者）
        // 使用子查询计算未读数：last_message_seq - last_read_msg_seq
        let query = if let Some(exclude_id) = exclude_user_id {
            sqlx::query(
                r#"
                UPDATE session_participants
                SET 
                    unread_count = GREATEST(0, $1 - COALESCE(last_read_msg_seq, 0)),
                    last_sync_msg_seq = GREATEST(last_sync_msg_seq, $1),
                    updated_at = CURRENT_TIMESTAMP
                WHERE session_id = $2 AND user_id != $3
                "#,
            )
            .bind(last_message_seq)
            .bind(session_id)
            .bind(exclude_id)
        } else {
            sqlx::query(
                r#"
                UPDATE session_participants
                SET 
                    unread_count = GREATEST(0, $1 - COALESCE(last_read_msg_seq, 0)),
                    last_sync_msg_seq = GREATEST(last_sync_msg_seq, $1),
                    updated_at = CURRENT_TIMESTAMP
                WHERE session_id = $2
                "#,
            )
            .bind(last_message_seq)
            .bind(session_id)
        };

        let result = query.execute(self.pool.as_ref()).await?;
        let rows_affected = result.rows_affected();

        debug!(
            session_id = %session_id,
            last_message_seq,
            exclude_user_id = ?exclude_user_id,
            rows_affected,
            "Batch updated participant unread counts"
        );

        Ok(())
    }
}

