use anyhow::Result;
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose};
use chrono::Utc;
use flare_im_core::utils::timestamp_to_datetime;
use flare_proto::common::{ContentType, Message, MessageSource, MessageStatus, MessageType};
use prost::Message as _;
use serde_json::to_value;
use sqlx::{Pool, Postgres, Row, postgres::PgPoolOptions};

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

        // 优化连接池配置（根据配置参数）
        let pool = PgPoolOptions::new()
            .max_connections(config.postgres_max_connections)
            .min_connections(config.postgres_min_connections)
            .acquire_timeout(std::time::Duration::from_secs(
                config.postgres_acquire_timeout_seconds,
            ))
            .idle_timeout(Some(std::time::Duration::from_secs(
                config.postgres_idle_timeout_seconds,
            )))
            .max_lifetime(Some(std::time::Duration::from_secs(
                config.postgres_max_lifetime_seconds,
            )))
            .test_before_acquire(true) // 获取连接前测试连接是否有效
            .connect(url)
            .await?;

        let store = Self { pool };

        // 初始化数据库表结构
        store
            .init_schema()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to initialize PostgreSQL schema: {}", e))?;

        Ok(Some(store))
    }
}

impl PostgresMessageStore {
    /// 获取数据库连接池（用于创建 SeqGenerator 和 ConversationRepository）
    pub fn pool(&self) -> &Pool<Postgres> {
        &self.pool
    }

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
                conversation_id TEXT NOT NULL,
                client_msg_id TEXT,
                sender_id TEXT NOT NULL,
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
                seq BIGINT,
                updated_at TIMESTAMP WITH TIME ZONE,
                visibility JSONB,
                read_by JSONB,
                operations JSONB,
                edit_history JSONB DEFAULT '[]'::jsonb,  -- 编辑历史记录
                reactions JSONB DEFAULT '[]'::jsonb,    -- 反应列表
                PRIMARY KEY (timestamp, id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // 创建索引（与 deploy/init.sql 保持一致）
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_messages_conversation_id 
            ON messages(conversation_id)
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
            ON messages(conversation_id, timestamp DESC)
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
            CREATE INDEX IF NOT EXISTS idx_messages_client_msg_id 
            ON messages(client_msg_id) WHERE client_msg_id IS NOT NULL
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_messages_sender_client_msg_id 
            ON messages(sender_id, client_msg_id) WHERE client_msg_id IS NOT NULL
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

        // 为 seq 字段创建索引（用于按会话查询消息时排序）
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_messages_session_seq 
            ON messages(conversation_id, seq)
            "#,
        )
        .execute(&self.pool)
        .await?;

        // 为 updated_at 字段创建索引（用于查询最近更新的消息）
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_messages_updated_at 
            ON messages(updated_at DESC)
            "#,
        )
        .execute(&self.pool)
        .await?;

        // 如果使用 TimescaleDB，转换为 Hypertable（与 deploy/init.sql 保持一致）
        let hypertable_result = sqlx::query(
            r#"
            SELECT create_hypertable('messages', 'timestamp', 
                chunk_time_interval => INTERVAL '1 day',
                if_not_exists => TRUE)
            "#,
        )
        .execute(&self.pool)
        .await;

        // 如果成功创建 hypertable，配置 TimescaleDB 高级特性
        if hypertable_result.is_ok() {
            // 启用列式存储（TimescaleDB 2.x+）
            // 注意：列式存储可以提高压缩比（约 10:1），但查询性能可能略有下降
            // 对于历史数据（30天以上），压缩带来的存储节省远大于查询性能损失
            let _ = sqlx::query(
                r#"
                ALTER TABLE messages SET (
                    timescaledb.enable_columnstore = true,
                    timescaledb.segmentby = 'conversation_id',
                    timescaledb.orderby = 'timestamp DESC, id'
                )
                "#,
            )
            .execute(&self.pool)
            .await; // 忽略错误，可能版本不支持或已配置

            // 配置列式存储策略（30天后移动到列式存储）
            // 注意：如果 TimescaleDB 版本 < 2.x，此策略会失败，但不影响基本功能
            let _ = sqlx::query(
                r#"
                CALL add_columnstore_policy('messages', after => INTERVAL '30 days')
                "#,
            )
            .execute(&self.pool)
            .await; // 忽略错误，可能版本不支持或已配置

            // 配置数据保留策略（可选，保留最近 90 天的数据）
            // 注意：生产环境可以根据需求调整保留时间
            // let _ = sqlx::query(
            //     r#"
            //     SELECT add_retention_policy('messages', INTERVAL '90 days')
            //     "#,
            // )
            // .execute(&self.pool)
            // .await;

            tracing::info!(
                "TimescaleDB hypertable configured with columnstore and compression policies"
            );
        } else {
            tracing::debug!(
                "Not using TimescaleDB or hypertable already exists, skipping advanced configuration"
            );
        }

        Ok(())
    }
}

#[async_trait]
impl ArchiveStoreRepository for PostgresMessageStore {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn store_archive(&self, message: &Message) -> Result<()> {
        let timestamp = message
            .timestamp
            .as_ref()
            .and_then(|ts| timestamp_to_datetime(ts))
            .unwrap_or_else(|| Utc::now());

        // 提取租户ID（保留用于 future use）
        let _tenant_id = message
            .tenant
            .as_ref()
            .map(|t| t.tenant_id.as_str())
            .unwrap_or("");

        // 推断 content_type（基于新的 MessageContent oneof 结构）
        let content_type = message
            .content
            .as_ref()
            .map(|c| match &c.content {
                Some(flare_proto::common::message_content::Content::Text(_)) => "text/plain",
                Some(flare_proto::common::message_content::Content::Image(_)) => "image/*",
                Some(flare_proto::common::message_content::Content::Video(_)) => "video/*",
                Some(flare_proto::common::message_content::Content::Audio(_)) => "audio/*",
                Some(flare_proto::common::message_content::Content::File(_)) => {
                    "application/octet-stream"
                }
                Some(flare_proto::common::message_content::Content::Location(_)) => {
                    "application/location"
                }
                Some(flare_proto::common::message_content::Content::Card(_)) => "application/card",
                Some(flare_proto::common::message_content::Content::Notification(_)) => {
                    "application/notification"
                }
                Some(flare_proto::common::message_content::Content::Custom(_)) => {
                    "application/custom"
                }
                Some(flare_proto::common::message_content::Content::Forward(_)) => {
                    "application/forward"
                }
                Some(flare_proto::common::message_content::Content::Typing(_)) => {
                    "application/typing"
                }
                Some(flare_proto::common::message_content::Content::SystemEvent(_)) => {
                    "application/system_event"
                }
                Some(flare_proto::common::message_content::Content::Quote(_)) => {
                    "application/quote"
                }
                Some(flare_proto::common::message_content::Content::LinkCard(_)) => {
                    "application/link_card"
                }
                None => "application/unknown",
            })
            .unwrap_or_else(|| {
                // 如果 content_type 字段有值，使用它
                match message.content_type {
                    x if x == ContentType::PlainText as i32 => "text/plain",
                    x if x == ContentType::Html as i32 => "text/html",
                    x if x == ContentType::Markdown as i32 => "text/markdown",
                    x if x == ContentType::Html as i32 => "text/html",
                    x if x == ContentType::Json as i32 => "application/json",
                    _ => "application/unknown",
                }
            });

        // 编码消息内容
        let content_bytes = message
            .content
            .as_ref()
            .map(|c| {
                let mut buf = Vec::new();
                c.encode(&mut buf).unwrap_or_default();
                buf
            })
            .unwrap_or_default();

        // 构建 extra JSONB 字段（包含所有扩展信息）
        let mut extra_value = serde_json::Map::new();
        if let Some(ref tenant) = message.tenant {
            extra_value.insert(
                "tenant_id".to_string(),
                serde_json::Value::String(tenant.tenant_id.clone()),
            );
        }
        // 转换 source 枚举为字符串（sender_type 已改为 source 枚举）
        let source_str = match std::convert::TryFrom::try_from(message.source) {
            Ok(MessageSource::User) => "user",
            Ok(MessageSource::System) => "system",
            Ok(MessageSource::Bot) => "bot",
            Ok(MessageSource::Admin) => "admin",
            _ => "",
        };
        if !source_str.is_empty() {
            extra_value.insert(
                "sender_type".to_string(),
                serde_json::Value::String(source_str.to_string()),
            );
        }
        // receiver_id 已废弃，通过 conversation_id 确定接收者
        // conversation_type 是 i32 类型，需要转换
        if message.conversation_type != 0 {
            let conversation_type_str = match message.conversation_type {
                1 => "single",
                2 => "group",
                3 => "channel",
                _ => "unknown",
            };
            extra_value.insert(
                "conversation_type".to_string(),
                serde_json::Value::String(conversation_type_str.to_string()),
            );
        }
        if !message.tags.is_empty() {
            extra_value.insert(
                "tags".to_string(),
                serde_json::Value::Array(
                    message
                        .tags
                        .iter()
                        .map(|t| serde_json::Value::String(t.clone()))
                        .collect(),
                ),
            );
        }
        if !message.attributes.is_empty() {
            for (k, v) in &message.attributes {
                extra_value.insert(k.clone(), serde_json::Value::String(v.clone()));
            }
        }
        // 合并原有的 extra 字段
        if let Ok(existing_extra) = serde_json::from_value::<
            serde_json::Map<String, serde_json::Value>,
        >(to_value(&message.extra)?)
        {
            for (k, v) in existing_extra {
                extra_value.insert(k, v);
            }
        }

        // 转换 message_type 枚举为字符串（与表结构匹配）
        let message_type_str = std::convert::TryFrom::try_from(message.message_type)
            .ok()
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

        // 从 extra 中提取 seq（如果存在）
        let seq: Option<i64> = extra_value.get("seq").and_then(|v| v.as_i64());

        sqlx::query(
            r#"
            INSERT INTO messages (
                server_id, conversation_id, client_msg_id, sender_id, content, timestamp,
                extra, created_at, message_type, content_type, business_type,
                status, is_recalled, recalled_at, is_burn_after_read, burn_after_seconds,
                seq, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $8)
            ON CONFLICT (timestamp, server_id) DO UPDATE
            SET conversation_id = EXCLUDED.conversation_id,
                client_msg_id = EXCLUDED.client_msg_id,
                sender_id = EXCLUDED.sender_id,
                content = EXCLUDED.content,
                content_type = EXCLUDED.content_type,
                status = EXCLUDED.status,
                extra = EXCLUDED.extra,
                business_type = EXCLUDED.business_type,
                message_type = EXCLUDED.message_type,
                seq = EXCLUDED.seq,
                updated_at = EXCLUDED.updated_at
            "#,
        )
        .bind(&message.server_id)
        .bind(&message.conversation_id)
        .bind(if message.client_msg_id.is_empty() { None::<String> } else { Some(message.client_msg_id.clone()) })
        .bind(&message.sender_id)
        .bind(content_bytes)
        .bind(timestamp)
        .bind(to_value(&extra_value)?)
        .bind(timestamp) // created_at 使用 timestamp
        .bind(message_type_str.as_deref())
        .bind(content_type)
        .bind(&message.business_type)
        .bind({
            // 转换 status 枚举为字符串
            std::convert::TryFrom::try_from(message.status)
                .ok()
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
        .bind(
            message
                .recalled_at
                .as_ref()
                .and_then(|ts| timestamp_to_datetime(ts)),
        )
        .bind(message.is_burn_after_read)
        .bind(message.burn_after_seconds)
        .bind(seq)
        .bind(timestamp) // updated_at 使用 timestamp
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn update_message_status(
        &self,
        message_id: &str,
        status: flare_proto::common::MessageStatus,
        is_recalled: Option<bool>,
        recalled_at: Option<prost_types::Timestamp>,
    ) -> Result<()> {
        self.update_message_status_internal(message_id, status, is_recalled, recalled_at)
            .await
    }

    async fn update_message_content(
        &self,
        message_id: &str,
        new_content: &flare_proto::common::MessageContent,
        edit_version: i32,
    ) -> Result<()> {
        self.update_message_content_internal(message_id, new_content, edit_version)
            .await
    }

    async fn update_message_visibility(
        &self,
        message_id: &str,
        user_id: Option<&str>,
        visibility: flare_proto::common::VisibilityStatus,
    ) -> Result<()> {
        self.update_message_visibility_internal(message_id, user_id, visibility)
            .await
    }

    async fn append_operation(
        &self,
        message_id: &str,
        operation: &flare_proto::common::MessageOperation,
    ) -> Result<()> {
        self.append_operation_internal(message_id, operation).await
    }

    /// 批量存储消息（优化性能）
    ///
    /// 使用 TimescaleDB 优化的批量插入策略：
    /// - 小批量（<=10）：逐个插入（简单可靠）
    /// - 中批量（11-500）：使用 VALUES 多行插入（单事务，性能较好）
    /// - 大批量（>500）：分批处理，每批最多 500 条（避免单次事务过大）
    ///
    /// 批量大小自适应：
    /// - 根据消息大小动态调整批量大小
    /// - 避免单次事务过大导致超时或内存问题
    async fn store_archive_batch(&self, messages: &[Message]) -> Result<()> {
        if messages.is_empty() {
            return Ok(());
        }

        // 小批量：逐个插入（简单且可靠）
        if messages.len() <= 10 {
            for message in messages {
                self.store_archive(message).await?;
            }
            return Ok(());
        }

        // 计算平均消息大小（用于自适应批量大小）
        let avg_message_size = messages
            .iter()
            .map(|m| {
                // 估算消息大小（content + extra + metadata）
                let content_size = m
                    .content
                    .as_ref()
                    .map(|c| {
                        let mut buf = Vec::new();
                        c.encode(&mut buf).unwrap_or_default();
                        buf.len()
                    })
                    .unwrap_or(0);
                let extra_size = serde_json::to_string(&m.extra).unwrap_or_default().len();
                content_size + extra_size + 200 // 基础元数据约 200 字节
            })
            .sum::<usize>()
            / messages.len();

        // 自适应批量大小：
        // - 小消息（<1KB）：每批最多 500 条
        // - 中等消息（1-10KB）：每批最多 200 条
        // - 大消息（>10KB）：每批最多 50 条
        let optimal_batch_size = if avg_message_size < 1024 {
            500
        } else if avg_message_size < 10 * 1024 {
            200
        } else {
            50
        };

        // 如果消息数量超过最优批量大小，分批处理
        if messages.len() > optimal_batch_size {
            let chunks: Vec<_> = messages.chunks(optimal_batch_size).collect();
            tracing::debug!(
                total_messages = messages.len(),
                optimal_batch_size = optimal_batch_size,
                chunks = chunks.len(),
                avg_message_size = avg_message_size,
                "Splitting batch into {} chunks for optimal performance",
                chunks.len()
            );

            for chunk in chunks {
                self.store_archive_batch_values(chunk).await?;
            }
            return Ok(());
        }

        // 中批量：使用 VALUES 多行插入（单事务，性能较好）
        self.store_archive_batch_values(messages).await
    }
}

impl PostgresMessageStore {
    /// 使用 VALUES 多行插入进行批量存储（优化版本）
    ///
    /// 此方法使用 sqlx::QueryBuilder 构建批量 INSERT 语句，
    /// 利用 TimescaleDB 的批量插入优化，性能比循环插入提升 10-100 倍
    ///
    /// 错误处理和重试：
    /// - 事务失败时自动重试（最多 3 次）
    /// - 使用指数退避策略
    async fn store_archive_batch_values(&self, messages: &[Message]) -> Result<()> {
        use sqlx::QueryBuilder;
        use std::time::Duration;

        // 预先处理所有消息，提取需要的数据（在重试循环外，避免重复计算）
        let prepared_data: Vec<_> = messages
            .iter()
            .map(|message| {
                let timestamp = message
                    .timestamp
                    .as_ref()
                    .and_then(|ts| timestamp_to_datetime(ts))
                    .unwrap_or_else(|| Utc::now());

                // 构建 extra JSONB 字段
                let mut extra_value = serde_json::Map::new();
                if let Some(ref tenant) = message.tenant {
                    extra_value.insert(
                        "tenant_id".to_string(),
                        serde_json::Value::String(tenant.tenant_id.clone()),
                    );
                }
                let source_str = match std::convert::TryFrom::try_from(message.source) {
                    Ok(MessageSource::User) => "user",
                    Ok(MessageSource::System) => "system",
                    Ok(MessageSource::Bot) => "bot",
                    Ok(MessageSource::Admin) => "admin",
                    _ => "",
                };
                if !source_str.is_empty() {
                    extra_value.insert(
                        "sender_type".to_string(),
                        serde_json::Value::String(source_str.to_string()),
                    );
                }
                // receiver_id 已废弃，通过 conversation_id 确定接收者
                // conversation_type 是 i32 类型，需要转换
                if message.conversation_type != 0 {
                    let conversation_type_str = match message.conversation_type {
                        1 => "single",
                        2 => "group",
                        3 => "channel",
                        _ => "unknown",
                    };
                    extra_value.insert(
                        "conversation_type".to_string(),
                        serde_json::Value::String(conversation_type_str.to_string()),
                    );
                }
                if !message.tags.is_empty() {
                    extra_value.insert(
                        "tags".to_string(),
                        serde_json::Value::Array(
                            message
                                .tags
                                .iter()
                                .map(|t| serde_json::Value::String(t.clone()))
                                .collect(),
                        ),
                    );
                }
                if !message.attributes.is_empty() {
                    for (k, v) in &message.attributes {
                        extra_value.insert(k.clone(), serde_json::Value::String(v.clone()));
                    }
                }
                if let Ok(existing_extra) =
                    serde_json::from_value::<serde_json::Map<String, serde_json::Value>>(
                        to_value(&message.extra).unwrap_or_default(),
                    )
                {
                    for (k, v) in existing_extra {
                        extra_value.insert(k, v);
                    }
                }

                let content_type = message
                    .content
                    .as_ref()
                    .map(|c| match &c.content {
                        Some(flare_proto::common::message_content::Content::Text(_)) => {
                            "text/plain"
                        }
                        Some(flare_proto::common::message_content::Content::Image(_)) => "image/*",
                        Some(flare_proto::common::message_content::Content::Video(_)) => "video/*",
                        Some(flare_proto::common::message_content::Content::Audio(_)) => "audio/*",
                        Some(flare_proto::common::message_content::Content::File(_)) => {
                            "application/octet-stream"
                        }
                        Some(flare_proto::common::message_content::Content::Location(_)) => {
                            "application/location"
                        }
                        Some(flare_proto::common::message_content::Content::Card(_)) => {
                            "application/card"
                        }
                        Some(flare_proto::common::message_content::Content::Notification(_)) => {
                            "application/notification"
                        }
                        Some(flare_proto::common::message_content::Content::Custom(_)) => {
                            "application/custom"
                        }
                        Some(flare_proto::common::message_content::Content::Forward(_)) => {
                            "application/forward"
                        }
                        Some(flare_proto::common::message_content::Content::Typing(_)) => {
                            "application/typing"
                        }
                        Some(flare_proto::common::message_content::Content::Custom(c)) => {
                            // Vote/Task/Schedule/Announcement 现在通过 Custom + content_type 实现
                            match c.r#type.as_str() {
                                "vote" => "application/vote",
                                "task" => "application/task",
                                "schedule" => "application/schedule",
                                "announcement" => "application/announcement",
                                _ => "application/custom",
                            }
                        }
                        Some(flare_proto::common::message_content::Content::SystemEvent(_)) => {
                            "application/system_event"
                        }
                        Some(flare_proto::common::message_content::Content::Quote(_)) => {
                            "application/quote"
                        }
                        Some(flare_proto::common::message_content::Content::LinkCard(_)) => {
                            "application/link_card"
                        }
                        None => "application/unknown",
                    })
                    .unwrap_or_else(|| match message.content_type {
                        x if x == ContentType::PlainText as i32 => "text/plain",
                        x if x == ContentType::Html as i32 => "text/html",
                        x if x == ContentType::Markdown as i32 => "text/markdown",
                        x if x == ContentType::Html as i32 => "text/html",
                        x if x == ContentType::Json as i32 => "application/json",
                        _ => "application/unknown",
                    });

                let content_bytes = message
                    .content
                    .as_ref()
                    .map(|c| {
                        let mut buf = Vec::new();
                        c.encode(&mut buf).unwrap_or_default();
                        buf
                    })
                    .unwrap_or_default();

                let message_type_str = std::convert::TryFrom::try_from(message.message_type)
                    .ok()
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

                let seq: Option<i64> = extra_value.get("seq").and_then(|v| v.as_i64());

                let status_str = std::convert::TryFrom::try_from(message.status)
                    .ok()
                    .map(|s| match s {
                        MessageStatus::Created => "created",
                        MessageStatus::Sent => "sent",
                        MessageStatus::Delivered => "delivered",
                        MessageStatus::Read => "read",
                        MessageStatus::Failed => "failed",
                        MessageStatus::Recalled => "recalled",
                        _ => "unknown",
                    })
                    .unwrap_or("unknown");

                (
                    message.server_id.clone(),
                    message.conversation_id.clone(),
                    if message.client_msg_id.is_empty() { None } else { Some(message.client_msg_id.clone()) },
                    message.sender_id.clone(),
                    content_bytes,
                    timestamp,
                    to_value(&extra_value).unwrap_or_default(),
                    message_type_str,
                    content_type.to_string(),
                    message.business_type.clone(),
                    status_str.to_string(),
                    message.is_recalled,
                    message
                        .recalled_at
                        .as_ref()
                        .and_then(|ts| timestamp_to_datetime(ts)),
                    message.is_burn_after_read,
                    message.burn_after_seconds,
                    seq,
                )
            })
            .collect();

        // 重试机制（最多 3 次）
        let max_retries = 3;
        let mut last_error: Option<anyhow::Error> = None;

        for attempt in 0..max_retries {
            // 使用事务确保原子性
            let mut tx = match self.pool.begin().await {
                Ok(tx) => tx,
                Err(e) => {
                    last_error = Some(anyhow::Error::from(e));
                    if attempt < max_retries - 1 {
                        // 指数退避：1s, 2s, 4s
                        let backoff = Duration::from_millis(1000 * (1 << attempt));
                        tracing::warn!(
                            attempt = attempt + 1,
                            backoff_ms = backoff.as_millis(),
                            "Failed to begin transaction, retrying after backoff"
                        );
                        tokio::time::sleep(backoff).await;
                        continue;
                    }
                    return Err(last_error.unwrap());
                }
            };

            // 构建批量 INSERT 语句
            let mut query_builder: QueryBuilder<Postgres> = QueryBuilder::new(
                r#"
                INSERT INTO messages (
                    server_id, conversation_id, client_msg_id, sender_id, content, timestamp,
                    extra, created_at, message_type, content_type, business_type,
                    status, is_recalled, recalled_at, is_burn_after_read, burn_after_seconds,
                    seq, updated_at
                )
                "#,
            );

            query_builder.push_values(&prepared_data, |mut b, row| {
                b.push_bind(&row.0); // server_id
                b.push_bind(&row.1); // conversation_id
                b.push_bind(&row.2); // client_msg_id (Option<String>)
                b.push_bind(&row.3); // sender_id
                b.push_bind(&row.4); // content_bytes
                b.push_bind(row.5); // timestamp
                b.push_bind(&row.6); // extra
                b.push_bind(row.5); // created_at (same as timestamp)
                b.push_bind(row.7.as_deref()); // message_type_str
                b.push_bind(&row.8); // content_type
                b.push_bind(&row.9); // business_type
                b.push_bind(&row.10); // status_str
                b.push_bind(row.11); // is_recalled
                b.push_bind(row.12); // recalled_at
                b.push_bind(row.13); // is_burn_after_read
                b.push_bind(row.14); // burn_after_seconds
                b.push_bind(row.15); // seq
                b.push_bind(row.5); // updated_at (same as timestamp)
            });

            query_builder.push(
                r#"
                ON CONFLICT (timestamp, server_id) DO UPDATE
                SET conversation_id = EXCLUDED.conversation_id,
                    client_msg_id = EXCLUDED.client_msg_id,
                    sender_id = EXCLUDED.sender_id,
                    content = EXCLUDED.content,
                    content_type = EXCLUDED.content_type,
                    status = EXCLUDED.status,
                    extra = EXCLUDED.extra,
                    business_type = EXCLUDED.business_type,
                    message_type = EXCLUDED.message_type,
                    seq = EXCLUDED.seq,
                    updated_at = EXCLUDED.updated_at
                "#,
            );

            // 执行批量插入
            match query_builder.build().execute(&mut *tx).await {
                Ok(_) => {
                    // 提交事务
                    match tx.commit().await {
                        Ok(_) => {
                            tracing::info!(
                                batch_size = messages.len(),
                                attempt = attempt + 1,
                                "Successfully batch inserted {} messages into TimescaleDB using VALUES",
                                messages.len()
                            );
                            return Ok(());
                        }
                        Err(e) => {
                            last_error = Some(anyhow::Error::from(e));
                            if attempt < max_retries - 1 {
                                let backoff = Duration::from_millis(1000 * (1 << attempt));
                                tracing::warn!(
                                    attempt = attempt + 1,
                                    backoff_ms = backoff.as_millis(),
                                    "Failed to commit transaction, retrying after backoff"
                                );
                                tokio::time::sleep(backoff).await;
                                continue;
                            }
                        }
                    }
                }
                Err(e) => {
                    last_error = Some(anyhow::Error::from(e));
                    // 回滚事务
                    let _ = tx.rollback().await;
                    if attempt < max_retries - 1 {
                        let backoff = Duration::from_millis(1000 * (1 << attempt));
                        tracing::warn!(
                            attempt = attempt + 1,
                            backoff_ms = backoff.as_millis(),
                            "Failed to execute batch insert, retrying after backoff"
                        );
                        tokio::time::sleep(backoff).await;
                        continue;
                    }
                }
            }
        }

        // 所有重试都失败
        Err(anyhow::anyhow!(
            "Failed to batch insert {} messages after {} attempts: {}",
            messages.len(),
            max_retries,
            last_error
                .map(|e| e.to_string())
                .unwrap_or_else(|| "Unknown error".to_string())
        ))
    }
}

impl PostgresMessageStore {
    /// 批量存储消息（内部方法，用于循环插入的旧实现，保留作为备用）
    #[allow(dead_code)]
    async fn store_archive_batch_legacy(&self, messages: &[Message]) -> Result<()> {
        // 大批量：使用事务批量插入（旧实现，保留作为备用）
        let mut tx = self.pool.begin().await?;

        for message in messages {
            let timestamp = message
                .timestamp
                .as_ref()
                .and_then(|ts| timestamp_to_datetime(ts))
                .unwrap_or_else(|| Utc::now());

            // 构建 extra JSONB 字段（复用单条插入的逻辑）
            let mut extra_value = serde_json::Map::new();
            if let Some(ref tenant) = message.tenant {
                extra_value.insert(
                    "tenant_id".to_string(),
                    serde_json::Value::String(tenant.tenant_id.clone()),
                );
            }
            let source_str = match std::convert::TryFrom::try_from(message.source) {
                Ok(MessageSource::User) => "user",
                Ok(MessageSource::System) => "system",
                Ok(MessageSource::Bot) => "bot",
                Ok(MessageSource::Admin) => "admin",
                _ => "",
            };
            if !source_str.is_empty() {
                extra_value.insert(
                    "sender_type".to_string(),
                    serde_json::Value::String(source_str.to_string()),
                );
            }
            // receiver_id 已废弃，通过 conversation_id 确定接收者
            // conversation_type 是 i32 类型，需要转换
            if message.conversation_type != 0 {
                let conversation_type_str = match message.conversation_type {
                    1 => "single",
                    2 => "group",
                    3 => "channel",
                    _ => "unknown",
                };
                extra_value.insert(
                    "conversation_type".to_string(),
                    serde_json::Value::String(conversation_type_str.to_string()),
                );
            }
            if !message.tags.is_empty() {
                extra_value.insert(
                    "tags".to_string(),
                    serde_json::Value::Array(
                        message
                            .tags
                            .iter()
                            .map(|t| serde_json::Value::String(t.clone()))
                            .collect(),
                    ),
                );
            }
            if !message.attributes.is_empty() {
                for (k, v) in &message.attributes {
                    extra_value.insert(k.clone(), serde_json::Value::String(v.clone()));
                }
            }
            if let Ok(existing_extra) = serde_json::from_value::<
                serde_json::Map<String, serde_json::Value>,
            >(to_value(&message.extra)?)
            {
                for (k, v) in existing_extra {
                    extra_value.insert(k, v);
                }
            }

            let content_type = message
                .content
                .as_ref()
                .map(|c| match &c.content {
                    Some(flare_proto::common::message_content::Content::Text(_)) => "text/plain",
                    Some(flare_proto::common::message_content::Content::Image(_)) => "image/*",
                    Some(flare_proto::common::message_content::Content::Video(_)) => "video/*",
                    Some(flare_proto::common::message_content::Content::Audio(_)) => "audio/*",
                    Some(flare_proto::common::message_content::Content::File(_)) => {
                        "application/octet-stream"
                    }
                    Some(flare_proto::common::message_content::Content::Location(_)) => {
                        "application/location"
                    }
                    Some(flare_proto::common::message_content::Content::Card(_)) => {
                        "application/card"
                    }
                    Some(flare_proto::common::message_content::Content::Notification(_)) => {
                        "application/notification"
                    }
                    Some(flare_proto::common::message_content::Content::Custom(_)) => {
                        "application/custom"
                    }
                    Some(flare_proto::common::message_content::Content::Forward(_)) => {
                        "application/forward"
                    }
                    Some(flare_proto::common::message_content::Content::Typing(_)) => {
                        "application/typing"
                    }
                    Some(flare_proto::common::message_content::Content::Custom(c)) => {
                        // Vote/Task/Schedule/Announcement 现在通过 Custom + content_type 实现
                        match c.r#type.as_str() {
                            "vote" => "application/vote",
                            "task" => "application/task",
                            "schedule" => "application/schedule",
                            "announcement" => "application/announcement",
                            _ => "application/custom",
                        }
                    }
                    Some(flare_proto::common::message_content::Content::SystemEvent(_)) => {
                        "application/system_event"
                    }
                    Some(flare_proto::common::message_content::Content::Quote(_)) => {
                        "application/quote"
                    }
                    Some(flare_proto::common::message_content::Content::LinkCard(_)) => {
                        "application/link_card"
                    }
                    None => "application/unknown",
                })
                .unwrap_or_else(|| match message.content_type {
                    x if x == ContentType::PlainText as i32 => "text/plain",
                    x if x == ContentType::Html as i32 => "text/html",
                    x if x == ContentType::Markdown as i32 => "text/markdown",
                    x if x == ContentType::Html as i32 => "text/html",
                    x if x == ContentType::Json as i32 => "application/json",
                    _ => "application/unknown",
                });

            let content_bytes = message
                .content
                .as_ref()
                .map(|c| {
                    let mut buf = Vec::new();
                    c.encode(&mut buf).unwrap_or_default();
                    buf
                })
                .unwrap_or_default();

            let message_type_str = std::convert::TryFrom::try_from(message.message_type)
                .ok()
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

            let seq: Option<i64> = extra_value.get("seq").and_then(|v| v.as_i64());

            let status_str = std::convert::TryFrom::try_from(message.status)
                .ok()
                .map(|s| match s {
                    MessageStatus::Created => "created",
                    MessageStatus::Sent => "sent",
                    MessageStatus::Delivered => "delivered",
                    MessageStatus::Read => "read",
                    MessageStatus::Failed => "failed",
                    MessageStatus::Recalled => "recalled",
                    _ => "unknown",
                })
                .unwrap_or("unknown");

            sqlx::query(
                r#"
                INSERT INTO messages (
                    server_id, conversation_id, sender_id, content, timestamp,
                    extra, created_at, message_type, content_type, business_type,
                    status, is_recalled, recalled_at, is_burn_after_read, burn_after_seconds,
                    seq, updated_at
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)
                ON CONFLICT (timestamp, server_id) DO UPDATE
                SET conversation_id = EXCLUDED.conversation_id,
                    sender_id = EXCLUDED.sender_id,
                    content = EXCLUDED.content,
                    content_type = EXCLUDED.content_type,
                    status = EXCLUDED.status,
                    extra = EXCLUDED.extra,
                    business_type = EXCLUDED.business_type,
                    message_type = EXCLUDED.message_type,
                    seq = EXCLUDED.seq,
                    updated_at = EXCLUDED.updated_at
                "#,
            )
            .bind(&message.server_id)
            .bind(&message.conversation_id)
            .bind(&message.sender_id)
            .bind(&content_bytes)
            .bind(timestamp)
            .bind(to_value(&extra_value)?)
            .bind(timestamp) // created_at
            .bind(message_type_str.as_deref())
            .bind(content_type)
            .bind(&message.business_type)
            .bind(status_str)
            .bind(message.is_recalled)
            .bind(message.recalled_at.as_ref().and_then(|ts| timestamp_to_datetime(ts)))
            .bind(message.is_burn_after_read)
            .bind(message.burn_after_seconds)
            .bind(seq)
            .bind(timestamp) // updated_at
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;

        tracing::info!(
            batch_size = messages.len(),
            "Successfully batch inserted {} messages into PostgreSQL",
            messages.len()
        );

        Ok(())
    }
}

impl PostgresMessageStore {
    /// 更新消息状态（用于撤回、编辑、删除等操作）
    async fn update_message_status_internal(
        &self,
        message_id: &str,
        status: flare_proto::common::MessageStatus,
        is_recalled: Option<bool>,
        recalled_at: Option<prost_types::Timestamp>,
    ) -> Result<()> {
        use sqlx::QueryBuilder;

        let status_str = match status {
            flare_proto::common::MessageStatus::Created => "created",
            flare_proto::common::MessageStatus::Sent => "sent",
            flare_proto::common::MessageStatus::Delivered => "delivered",
            flare_proto::common::MessageStatus::Read => "read",
            flare_proto::common::MessageStatus::Failed => "failed",
            flare_proto::common::MessageStatus::Recalled => "recalled",
            _ => "unknown",
        };

        let mut query = QueryBuilder::new("UPDATE messages SET status = ");
        query.push_bind(status_str);
        query.push(", updated_at = CURRENT_TIMESTAMP");

        if let Some(recalled) = is_recalled {
            query.push(", is_recalled = ");
            query.push_bind(recalled);
        }

        if let Some(recalled_ts) = recalled_at {
            if let Some(dt) = timestamp_to_datetime(&recalled_ts) {
                query.push(", recalled_at = ");
                query.push_bind(dt);
            }
        }

        query.push(" WHERE server_id = ");
        query.push_bind(message_id);

        query.build().execute(&self.pool).await?;

        Ok(())
    }

    /// 更新消息内容（用于编辑操作）
    ///
    /// 功能：
    /// 1. 获取当前消息内容
    /// 2. 将当前内容保存到编辑历史
    /// 3. 更新消息内容为新内容
    /// 4. 更新编辑历史列表
    async fn update_message_content_internal(
        &self,
        message_id: &str,
        new_content: &flare_proto::common::MessageContent,
        edit_version: i32,
    ) -> Result<()> {
        use prost::Message as _;

        // 1. 获取当前消息（用于保存编辑历史）
        let current_message_row = sqlx::query(
            r#"
            SELECT content, sender_id, timestamp, edit_history
            FROM messages
            WHERE server_id = $1
            "#,
        )
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await?;

        let (current_content_bytes, sender_id, timestamp, existing_edit_history_json) =
            match current_message_row {
                Some(row) => (
                    row.get::<Vec<u8>, _>("content"),
                    row.get::<String, _>("sender_id"),
                    row.get::<chrono::DateTime<chrono::Utc>, _>("timestamp"),
                    row.try_get::<serde_json::Value, _>("edit_history").ok(),
                ),
                None => return Err(anyhow::anyhow!("Message not found: {}", message_id)),
            };

        // 2. 验证编辑版本号（必须递增）
        let current_edit_version: i32 = existing_edit_history_json
            .as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.last())
            .and_then(|entry| entry.get("edit_version"))
            .and_then(|v| v.as_i64())
            .map(|v| v as i32)
            .unwrap_or(0);

        if edit_version <= current_edit_version {
            return Err(anyhow::anyhow!(
                "Edit version must be greater than current version. Current: {}, Provided: {}",
                current_edit_version,
                edit_version
            ));
        }

        // 4. 构建编辑历史记录
        // 注意：edit_history 存储为 JSONB，包含完整的 EditHistory 信息
        // 由于 MessageContent 是二进制格式，我们将其编码为 base64 字符串存储
        let content_base64 = general_purpose::STANDARD.encode(&current_content_bytes);

        let edit_history_entry = serde_json::json!({
            "edit_version": current_edit_version,  // 当前版本（编辑前的版本）
            "content_encoded": content_base64,     // base64 编码的内容
            "edited_at": timestamp.to_rfc3339(),
            "editor_id": sender_id,
            "reason": "",
            "show_edited_mark": true
        });

        // 5. 合并编辑历史
        let mut edit_history_array = match existing_edit_history_json {
            Some(serde_json::Value::Array(arr)) => arr,
            _ => Vec::new(),
        };
        edit_history_array.push(edit_history_entry);

        // 6. 编码新内容
        let mut new_content_bytes = Vec::new();
        new_content.encode(&mut new_content_bytes)?;

        // 7. 更新消息内容、编辑历史和 extra
        sqlx::query(
            r#"
            UPDATE messages
            SET 
                content = $1,
                edit_history = $2::jsonb,
                updated_at = CURRENT_TIMESTAMP,
                extra = COALESCE(extra, '{}'::jsonb) || $3::jsonb
            WHERE id = $4
            "#,
        )
        .bind(&new_content_bytes)
        .bind(serde_json::Value::Array(edit_history_array))
        .bind(serde_json::json!({
            "edit_version": edit_version,
            "edited_at": Utc::now().timestamp()
        }))
        .bind(message_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// 更新消息可见性（用于删除操作）
    async fn update_message_visibility_internal(
        &self,
        message_id: &str,
        user_id: Option<&str>,
        visibility: flare_proto::common::VisibilityStatus,
    ) -> Result<()> {
        let vis_value = visibility as i32;

        if let Some(uid) = user_id {
            // 用户维度：更新 visibility JSONB 中的特定用户
            let vis_json = serde_json::json!({ uid: vis_value });
            sqlx::query(
                r#"
                UPDATE messages
                SET 
                    visibility = COALESCE(visibility, '{}'::jsonb) || $1::jsonb,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = $2
                "#,
            )
            .bind(serde_json::to_value(&vis_json)?)
            .bind(message_id)
            .execute(&self.pool)
            .await?;
        } else {
            // 全局删除：设置所有用户的 visibility 为 DELETED
            // 注意：这里我们使用一个特殊的标记来表示全局删除
            sqlx::query(
                r#"
                UPDATE messages
                SET 
                    visibility = jsonb_build_object('*', $1),
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = $2
                "#,
            )
            .bind(vis_value)
            .bind(message_id)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    /// 追加操作记录到消息
    async fn append_operation_internal(
        &self,
        message_id: &str,
        operation: &flare_proto::common::MessageOperation,
    ) -> Result<()> {
        // 读取当前消息的 operations
        let row = sqlx::query("SELECT operations FROM messages WHERE id = $1")
            .bind(message_id)
            .fetch_optional(&self.pool)
            .await?;

        let mut operations: Vec<serde_json::Value> = if let Some(row) = row {
            let ops_value: Option<serde_json::Value> = row.get("operations");
            ops_value
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_default()
        } else {
            return Err(anyhow::anyhow!("Message not found: {}", message_id));
        };

        // 序列化新操作
        let operation_json = serde_json::json!({
            "operation_type": operation.operation_type,
            "target_message_id": operation.target_message_id,
            "operator_id": operation.operator_id,
            "timestamp": operation.timestamp.as_ref().map(|ts| {
                serde_json::json!({
                    "seconds": ts.seconds,
                    "nanos": ts.nanos
                })
            }),
            "show_notice": operation.show_notice,
            "notice_text": operation.notice_text,
            "target_user_id": operation.target_user_id,
            "metadata": operation.metadata,
        });

        operations.push(operation_json);

        // 更新 operations 数组
        sqlx::query(
            r#"
            UPDATE messages
            SET 
                operations = $1::jsonb,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = $2
            "#,
        )
        .bind(serde_json::to_value(&operations)?)
        .bind(message_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
