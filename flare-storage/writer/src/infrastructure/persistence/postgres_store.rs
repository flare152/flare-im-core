use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use flare_im_core::utils::timestamp_to_datetime;
use flare_proto::storage::Message;
use prost::Message as _;
use serde_json::to_value;
use sqlx::{Pool, Postgres, postgres::PgPoolOptions};

use crate::config::StorageWriterConfig;
use crate::domain::repositories::ArchiveStoreRepository;

pub struct PostgresMessageStore {
    pool: Pool<Postgres>,
}

impl PostgresMessageStore {
    pub async fn new(config: &StorageWriterConfig) -> Result<Option<Self>> {
        let url = match &config.postgres_url {
            Some(url) => url,
            None => return Ok(None),
        };

        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(url)
            .await?;

        Ok(Some(Self { pool }))
    }
}

impl PostgresMessageStore {
    /// 初始化数据库表结构（如果不存在）
    pub async fn init_schema(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS messages (
                message_id VARCHAR(64) PRIMARY KEY,
                tenant_id VARCHAR(64) NOT NULL DEFAULT '',
                session_id VARCHAR(64) NOT NULL,
                sender_id VARCHAR(64) NOT NULL,
                sender_type VARCHAR(32) NOT NULL DEFAULT 'user',
                receiver_id VARCHAR(64) DEFAULT '',
                receiver_ids TEXT[] DEFAULT '{}',
                content BYTEA NOT NULL,
                content_type VARCHAR(32) NOT NULL DEFAULT 'text/plain',
                message_type INTEGER NOT NULL DEFAULT 0,
                business_type VARCHAR(64) NOT NULL DEFAULT '',
                session_type VARCHAR(32) NOT NULL DEFAULT 'single',
                status VARCHAR(32) NOT NULL DEFAULT 'sent',
                timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                deleted_at TIMESTAMPTZ,
                is_recalled BOOLEAN DEFAULT FALSE,
                recalled_at TIMESTAMPTZ,
                is_burn_after_read BOOLEAN DEFAULT FALSE,
                burn_after_seconds INTEGER DEFAULT 0,
                extra JSONB DEFAULT '{}'::jsonb,
                tags TEXT[] DEFAULT '{}',
                attributes JSONB DEFAULT '{}'::jsonb
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // 创建索引
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_messages_tenant_session_time 
            ON messages (tenant_id, session_id, timestamp DESC)
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_messages_tenant_sender_time 
            ON messages (tenant_id, sender_id, timestamp DESC)
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_messages_session_id 
            ON messages (session_id, timestamp DESC)
            "#,
        )
        .execute(&self.pool)
        .await?;

        // 如果使用 TimescaleDB，转换为 Hypertable
        let _ = sqlx::query(
            r#"
            SELECT create_hypertable('messages', 'timestamp', 
                chunk_time_interval => INTERVAL '1 month',
                if_not_exists => TRUE)
            "#,
        )
        .execute(&self.pool)
        .await; // 忽略错误，可能不是 TimescaleDB

        Ok(())
    }
}

#[async_trait]
impl ArchiveStoreRepository for PostgresMessageStore {
    async fn store_archive(&self, message: &Message) -> Result<()> {
        let timestamp = message
            .timestamp
            .as_ref()
            .and_then(|ts| timestamp_to_datetime(ts))
            .unwrap_or_else(|| Utc::now());

        // 提取租户ID
        let tenant_id = message
            .tenant
            .as_ref()
            .map(|t| t.tenant_id.as_str())
            .unwrap_or("");

        // 推断 content_type
        let content_type = message.content.as_ref()
            .map(|c| match &c.content {
                Some(flare_proto::storage::message_content::Content::Text(_)) => "text/plain",
                Some(flare_proto::storage::message_content::Content::Binary(_)) => "application/octet-stream",
                Some(flare_proto::storage::message_content::Content::Json(_)) => "application/json",
                Some(flare_proto::storage::message_content::Content::Custom(_)) => "application/custom",
                None => "application/unknown",
            })
            .unwrap_or("application/unknown");

        // 编码消息内容
        let content_bytes = message.content.as_ref()
            .map(|c| {
                let mut buf = Vec::new();
                c.encode(&mut buf).unwrap_or_default();
                buf
            })
            .unwrap_or_default();

        sqlx::query(
            r#"
            INSERT INTO messages (
                message_id, tenant_id, session_id, sender_id, sender_type,
                receiver_id, receiver_ids, content, content_type, message_type,
                business_type, session_type, status, timestamp,
                is_recalled, recalled_at, is_burn_after_read, burn_after_seconds,
                extra, tags, attributes
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21)
            ON CONFLICT (message_id) DO UPDATE
            SET session_id = EXCLUDED.session_id,
                sender_id = EXCLUDED.sender_id,
                receiver_id = EXCLUDED.receiver_id,
                receiver_ids = EXCLUDED.receiver_ids,
                content = EXCLUDED.content,
                content_type = EXCLUDED.content_type,
                status = EXCLUDED.status,
                extra = EXCLUDED.extra,
                updated_at = NOW()
            "#,
        )
        .bind(&message.id)
        .bind(tenant_id)
        .bind(&message.session_id)
        .bind(&message.sender_id)
        .bind(&message.sender_type)
        .bind(&message.receiver_id)
        .bind(to_value(&message.receiver_ids)?)
        .bind(content_bytes)
        .bind(content_type)
        .bind(message.message_type)
        .bind(&message.business_type)
        .bind(&message.session_type)
        .bind(&message.status)
        .bind(timestamp)
        .bind(message.is_recalled)
        .bind(message.recalled_at.as_ref().and_then(|ts| timestamp_to_datetime(ts)))
        .bind(message.is_burn_after_read)
        .bind(message.burn_after_seconds)
        .bind(to_value(&message.extra)?)
        .bind(to_value(&message.tags)?)
        .bind(to_value(&message.attributes)?)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
