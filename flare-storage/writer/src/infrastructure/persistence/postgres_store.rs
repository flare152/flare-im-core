use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use flare_im_core::utils::timestamp_to_datetime;
use flare_proto::common::{Message, MessageType, MessageSource, MessageStatus, ContentType};
use prost::Message as _;
use serde_json::to_value;
use sqlx::{Pool, Postgres, postgres::PgPoolOptions};

use crate::config::StorageWriterConfig;
use crate::domain::repository::ArchiveStoreRepository;

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

        let store = Self { pool };
        
        // 初始化数据库表结构
        store.init_schema().await
            .map_err(|e| anyhow::anyhow!("Failed to initialize PostgreSQL schema: {}", e))?;

        Ok(Some(store))
    }
}

impl PostgresMessageStore {
    /// 初始化数据库表结构（如果不存在）
    /// 
    /// 注意：表结构必须与 deploy/init.sql 中的定义一致
    /// 如果表已存在（通过 init.sql 创建），此方法不会修改表结构
    pub async fn init_schema(&self) -> Result<()> {
        // 创建 messages 表（与 deploy/init.sql 保持一致）
        // 注意：TimescaleDB 要求分区列（timestamp）必须包含在主键中
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS messages (
                id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                sender_id TEXT NOT NULL,
                receiver_ids JSONB,
                content BYTEA,
                timestamp TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
                extra JSONB DEFAULT '{}',
                created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
                message_type TEXT,
                content_type TEXT,
                business_type TEXT,
                status TEXT DEFAULT 'sent',
                is_recalled BOOLEAN DEFAULT FALSE,
                recalled_at TIMESTAMP WITH TIME ZONE,
                is_burn_after_read BOOLEAN DEFAULT FALSE,
                burn_after_seconds INTEGER,
                visibility JSONB,
                read_by JSONB,
                operations JSONB,
                PRIMARY KEY (timestamp, id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // 创建索引（与 deploy/init.sql 保持一致）
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_messages_session_id 
            ON messages(session_id)
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_messages_sender_id 
            ON messages(sender_id)
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_messages_session_timestamp 
            ON messages(session_id, timestamp DESC)
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_messages_id_unique 
            ON messages(id)
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_messages_business_type 
            ON messages(business_type)
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_messages_message_type 
            ON messages(message_type)
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_messages_status 
            ON messages(status)
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_messages_is_recalled 
            ON messages(is_recalled)
            "#,
        )
        .execute(&self.pool)
        .await?;

        // 如果使用 TimescaleDB，转换为 Hypertable（与 deploy/init.sql 保持一致）
        let _ = sqlx::query(
            r#"
            SELECT create_hypertable('messages', 'timestamp', 
                chunk_time_interval => INTERVAL '1 day',
                if_not_exists => TRUE)
            "#,
        )
        .execute(&self.pool)
        .await; // 忽略错误，可能不是 TimescaleDB 或表已存在

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

        // 推断 content_type（基于新的 MessageContent oneof 结构）
        let content_type = message.content.as_ref()
            .map(|c| match &c.content {
                Some(flare_proto::common::message_content::Content::Text(_)) => "text/plain",
                Some(flare_proto::common::message_content::Content::Image(_)) => "image/*",
                Some(flare_proto::common::message_content::Content::Video(_)) => "video/*",
                Some(flare_proto::common::message_content::Content::Audio(_)) => "audio/*",
                Some(flare_proto::common::message_content::Content::File(_)) => "application/octet-stream",
                Some(flare_proto::common::message_content::Content::Location(_)) => "application/location",
                Some(flare_proto::common::message_content::Content::Card(_)) => "application/card",
                Some(flare_proto::common::message_content::Content::Notification(_)) => "application/notification",
                Some(flare_proto::common::message_content::Content::Custom(_)) => "application/custom",
                Some(flare_proto::common::message_content::Content::Forward(_)) => "application/forward",
                Some(flare_proto::common::message_content::Content::Typing(_)) => "application/typing",
                None => "application/unknown",
            })
            .unwrap_or_else(|| {
                // 如果 content_type 字段有值，使用它
                match message.content_type {
                    x if x == ContentType::PlainText as i32 => "text/plain",
                    x if x == ContentType::RichText as i32 => "text/html",
                    x if x == ContentType::Markdown as i32 => "text/markdown",
                    x if x == ContentType::Html as i32 => "text/html",
                    x if x == ContentType::Json as i32 => "application/json",
                    _ => "application/unknown",
                }
            });

        // 编码消息内容
        let content_bytes = message.content.as_ref()
            .map(|c| {
                let mut buf = Vec::new();
                c.encode(&mut buf).unwrap_or_default();
                buf
            })
            .unwrap_or_default();

        // 构建 extra JSONB 字段（包含所有扩展信息）
        let mut extra_value = serde_json::Map::new();
        if let Some(ref tenant) = message.tenant {
            extra_value.insert("tenant_id".to_string(), serde_json::Value::String(tenant.tenant_id.clone()));
        }
        // 转换 source 枚举为字符串（sender_type 已改为 source 枚举）
        let source_str = match MessageSource::from_i32(message.source) {
            Some(MessageSource::User) => "user",
            Some(MessageSource::System) => "system",
            Some(MessageSource::Bot) => "bot",
            Some(MessageSource::Admin) => "admin",
            _ => "",
        };
        if !source_str.is_empty() {
            extra_value.insert("sender_type".to_string(), serde_json::Value::String(source_str.to_string()));
        }
        if !message.receiver_id.is_empty() {
            extra_value.insert("receiver_id".to_string(), serde_json::Value::String(message.receiver_id.clone()));
        }
        if !message.session_type.is_empty() {
            extra_value.insert("session_type".to_string(), serde_json::Value::String(message.session_type.clone()));
        }
        if !message.tags.is_empty() {
            extra_value.insert("tags".to_string(), serde_json::Value::Array(
                message.tags.iter().map(|t| serde_json::Value::String(t.clone())).collect()
            ));
        }
        if !message.attributes.is_empty() {
            for (k, v) in &message.attributes {
                extra_value.insert(k.clone(), serde_json::Value::String(v.clone()));
            }
        }
        // 合并原有的 extra 字段
        if let Ok(existing_extra) = serde_json::from_value::<serde_json::Map<String, serde_json::Value>>(to_value(&message.extra)?) {
            for (k, v) in existing_extra {
                extra_value.insert(k, v);
            }
        }

        // 转换 message_type 枚举为字符串（与表结构匹配）
        let message_type_str = MessageType::from_i32(message.message_type)
            .map(|mt| match mt {
                MessageType::Text => "text",
                MessageType::Image => "image",
                MessageType::Video => "video",
                MessageType::Audio => "audio",
                MessageType::File => "file",
                MessageType::Location => "location",
                MessageType::Card => "card",
                MessageType::Custom => "custom",
                MessageType::Notification => "notification",
                MessageType::Typing => "typing",
                MessageType::Recall => "recall",
                MessageType::Read => "read",
                MessageType::Forward => "forward",
                _ => "unknown",
            })
            .map(|s| s.to_string());

        sqlx::query(
            r#"
            INSERT INTO messages (
                id, session_id, sender_id, receiver_ids, content, timestamp,
                extra, created_at, message_type, content_type, business_type,
                status, is_recalled, recalled_at, is_burn_after_read, burn_after_seconds
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
            ON CONFLICT (timestamp, id) DO UPDATE
            SET session_id = EXCLUDED.session_id,
                sender_id = EXCLUDED.sender_id,
                receiver_ids = EXCLUDED.receiver_ids,
                content = EXCLUDED.content,
                content_type = EXCLUDED.content_type,
                status = EXCLUDED.status,
                extra = EXCLUDED.extra,
                business_type = EXCLUDED.business_type,
                message_type = EXCLUDED.message_type
            "#,
        )
        .bind(&message.id)
        .bind(&message.session_id)
        .bind(&message.sender_id)
        .bind(to_value(&message.receiver_ids)?)
        .bind(content_bytes)
        .bind(timestamp)
        .bind(to_value(&extra_value)?)
        .bind(timestamp) // created_at 使用 timestamp
        .bind(message_type_str.as_deref())
        .bind(content_type)
        .bind(&message.business_type)
        .bind({
            // 转换 status 枚举为字符串
            MessageStatus::from_i32(message.status)
                .map(|s| match s {
                    MessageStatus::Created => "created",
                    MessageStatus::Sent => "sent",
                    MessageStatus::Delivered => "delivered",
                    MessageStatus::Read => "read",
                    MessageStatus::Failed => "failed",
                    MessageStatus::Recalled => "recalled",
                    _ => "unknown",
                })
                .unwrap_or("unknown")
        })
        .bind(message.is_recalled)
        .bind(message.recalled_at.as_ref().and_then(|ts| timestamp_to_datetime(ts)))
        .bind(message.is_burn_after_read)
        .bind(message.burn_after_seconds)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
