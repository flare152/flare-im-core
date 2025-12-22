//! 会话仓储实现
//!
//! 负责更新会话和参与者信息（last_message_seq、unread_count等）

use anyhow::Result;
use async_trait::async_trait;
use sqlx::PgPool;
use std::sync::Arc;
use tracing::{debug, instrument};

use crate::domain::repository::ConversationUpdateRepository;

/// PostgreSQL 会话仓储实现
pub struct PostgresConversationRepository {
    pool: Arc<PgPool>,
}

impl PostgresConversationRepository {
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ConversationUpdateRepository for PostgresConversationRepository {
    #[instrument(skip(self), fields(conversation_id = %conversation_id, message_id = %message_id, seq))]
    async fn update_last_message(
        &self,
        conversation_id: &str,
        message_id: &str,
        seq: i64,
    ) -> Result<()> {
        // 使用 UPSERT 模式：如果会话不存在，则创建；如果存在，则更新
        // 这样可以避免竞态条件，即使 Message Orchestrator 的异步创建还未完成也能正常工作
        sqlx::query(
            r#"
            INSERT INTO conversations (
                conversation_id, 
                conversation_type, 
                business_type,
                last_message_id,
                last_message_seq,
                created_at,
                updated_at
            )
            VALUES ($1, 'single', 'chat', $2, $3, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
            ON CONFLICT (conversation_id) DO UPDATE
            SET 
                last_message_id = EXCLUDED.last_message_id,
                last_message_seq = EXCLUDED.last_message_seq,
                updated_at = CURRENT_TIMESTAMP
            "#,
        )
        .bind(conversation_id)
        .bind(message_id)
        .bind(seq)
        .execute(self.pool.as_ref())
        .await?;

        debug!(
            conversation_id = %conversation_id,
            message_id = %message_id,
            seq,
            "Updated conversation last_message (UPSERT)"
        );

        Ok(())
    }

    #[instrument(skip(self), fields(conversation_id = %conversation_id))]
    async fn batch_update_unread_count(
        &self,
        conversation_id: &str,
        last_message_seq: i64,
        exclude_user_id: Option<&str>,
    ) -> Result<()> {
        // 批量更新所有参与者的未读数（排除发送者）
        // 使用子查询计算未读数：last_message_seq - last_read_msg_seq
        // 注意：表名是 conversation_participants（不是 session_participants）
        let query = if let Some(exclude_id) = exclude_user_id {
            sqlx::query(
                r#"
                UPDATE conversation_participants
                SET 
                    unread_count = GREATEST(0, $1 - COALESCE(last_read_msg_seq, 0)),
                    last_sync_msg_seq = GREATEST(last_sync_msg_seq, $1),
                    updated_at = CURRENT_TIMESTAMP
                WHERE conversation_id = $2 AND user_id != $3
                "#,
            )
            .bind(last_message_seq)
            .bind(conversation_id)
            .bind(exclude_id)
        } else {
            sqlx::query(
                r#"
                UPDATE conversation_participants
                SET 
                    unread_count = GREATEST(0, $1 - COALESCE(last_read_msg_seq, 0)),
                    last_sync_msg_seq = GREATEST(last_sync_msg_seq, $1),
                    updated_at = CURRENT_TIMESTAMP
                WHERE conversation_id = $2
                "#,
            )
            .bind(last_message_seq)
            .bind(conversation_id)
        };

        let result = query.execute(self.pool.as_ref()).await?;
        let rows_affected = result.rows_affected();

        debug!(
            conversation_id = %conversation_id,
            last_message_seq,
            exclude_user_id = ?exclude_user_id,
            rows_affected,
            "Batch updated participant unread counts"
        );

        Ok(())
    }
}
