//! PostgreSQL 读侧存储实现
//!
//! 基于 TimescaleDB/PostgreSQL 实现消息的查询、更新、搜索等功能
//! 与 Storage Writer 共享相同的数据库表结构

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use flare_im_core::utils::{datetime_to_timestamp, timestamp_to_datetime};
use flare_proto::common::{
    ContentType, Message, MessageOperation, MessageReadRecord, MessageSource, MessageStatus,
    MessageType, Reaction, VisibilityStatus,
};
use prost::Message as ProstMessage;
use prost_types::Timestamp;
use serde_json::{Value, from_value};
use sqlx::{Pool, Postgres, Row, postgres::PgPoolOptions};

use crate::config::StorageReaderConfig;
use crate::domain::model::MessageUpdate;
use crate::domain::repository::{MessageStorage, VisibilityStorage};
use crate::infrastructure::cache::RedisMessageCache;

/// PostgreSQL 消息存储实现（带 Redis 缓存）
pub struct PostgresMessageStorage {
    pool: Pool<Postgres>,
    cache: Option<Arc<RedisMessageCache>>,
}

impl PostgresMessageStorage {
    /// 创建新的 PostgreSQL 存储实例（带可选的 Redis 缓存）
    pub async fn new(config: &StorageReaderConfig) -> Result<Option<Self>> {
        let url = match &config.postgres_url {
            Some(url) => url,
            None => return Ok(None),
        };

        // 使用配置的连接池参数
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
            .test_before_acquire(true) // 连接池健康检查
            .connect(url)
            .await
            .context("Failed to connect to PostgreSQL")?;

        // 初始化 Redis 缓存（可选）
        let cache = if let Some(redis_url) = &config.redis_url {
            let client =
                redis::Client::open(redis_url.as_str()).context("Failed to create Redis client")?;
            Some(Arc::new(RedisMessageCache::new(Arc::new(client), config)))
        } else {
            None
        };

        let storage = Self { pool, cache };

        // 验证表结构（不创建，由 Writer 或 init.sql 创建）
        storage
            .verify_schema()
            .await
            .context("Failed to verify PostgreSQL schema")?;

        Ok(Some(storage))
    }

    /// 验证表结构是否存在，并创建必要的索引（如果不存在）
    async fn verify_schema(&self) -> Result<()> {
        // 检查 messages 表是否存在
        let exists: bool = sqlx::query_scalar(
            r#"
            SELECT EXISTS (
                SELECT FROM information_schema.tables 
                WHERE table_schema = 'public' 
                AND table_name = 'messages'
            )
            "#,
        )
        .fetch_one(&self.pool)
        .await
        .context("Failed to check if messages table exists")?;

        if !exists {
            return Err(anyhow::anyhow!(
                "messages table does not exist. Please run init.sql or ensure Storage Writer has initialized the schema"
            ));
        }

        // 创建必要的索引（如果不存在）以优化查询性能
        self.ensure_indexes()
            .await
            .context("Failed to create indexes")?;

        Ok(())
    }

    /// 确保必要的索引存在（用于优化查询性能）
    async fn ensure_indexes(&self) -> Result<()> {
        // 索引列表（与 Writer 保持一致，但使用 IF NOT EXISTS 避免冲突）
        let indexes = vec![
            (
                "idx_messages_session_id",
                "CREATE INDEX IF NOT EXISTS idx_messages_session_id ON messages(session_id)",
            ),
            (
                "idx_messages_sender_id",
                "CREATE INDEX IF NOT EXISTS idx_messages_sender_id ON messages(sender_id)",
            ),
            (
                "idx_messages_session_timestamp",
                "CREATE INDEX IF NOT EXISTS idx_messages_session_timestamp ON messages(session_id, timestamp DESC)",
            ),
            (
                "idx_messages_id_unique",
                "CREATE INDEX IF NOT EXISTS idx_messages_id_unique ON messages(id)",
            ),
            (
                "idx_messages_business_type",
                "CREATE INDEX IF NOT EXISTS idx_messages_business_type ON messages(business_type)",
            ),
            (
                "idx_messages_message_type",
                "CREATE INDEX IF NOT EXISTS idx_messages_message_type ON messages(message_type)",
            ),
            (
                "idx_messages_status",
                "CREATE INDEX IF NOT EXISTS idx_messages_status ON messages(status)",
            ),
            (
                "idx_messages_is_recalled",
                "CREATE INDEX IF NOT EXISTS idx_messages_is_recalled ON messages(is_recalled)",
            ),
            (
                "idx_messages_session_seq",
                "CREATE INDEX IF NOT EXISTS idx_messages_session_seq ON messages(session_id, seq)",
            ),
            (
                "idx_messages_timestamp",
                "CREATE INDEX IF NOT EXISTS idx_messages_timestamp ON messages(timestamp DESC)",
            ),
        ];

        for (name, sql) in indexes {
            sqlx::query(sql)
                .execute(&self.pool)
                .await
                .with_context(|| format!("Failed to create index: {}", name))?;
        }

        tracing::info!("All indexes verified/created successfully");
        Ok(())
    }

    /// 健康检查：验证数据库连接和基本查询
    pub async fn health_check(&self) -> Result<()> {
        // 简单的查询测试连接
        let _: i64 = sqlx::query_scalar("SELECT 1")
            .fetch_one(&self.pool)
            .await
            .context("Health check failed: database connection error")?;

        // 检查连接池状态
        let pool_size = self.pool.size();
        let idle_connections = self.pool.num_idle();

        tracing::debug!(
            pool_size = pool_size,
            idle_connections = idle_connections,
            "Database connection pool status"
        );

        Ok(())
    }

    /// 从数据库行转换为 Message protobuf
    fn row_to_message(&self, row: &sqlx::postgres::PgRow) -> Result<Message> {
        let id: String = row.get("id");
        let session_id: String = row.get("session_id");
        let sender_id: String = row.get("sender_id");
        let receiver_ids: Option<Value> = row.get("receiver_ids");
        let content: Option<Vec<u8>> = row.get("content");
        let timestamp: DateTime<Utc> = row.get("timestamp");
        let extra: Option<Value> = row.get("extra");
        let _created_at: Option<DateTime<Utc>> = row.get("created_at");
        let message_type: Option<String> = row.get("message_type");
        let content_type: Option<String> = row.get("content_type");
        let business_type: String = row.get("business_type");
        let status: String = row.get("status");
        let is_recalled: bool = row.get("is_recalled");
        let recalled_at: Option<DateTime<Utc>> = row.get("recalled_at");
        let is_burn_after_read: bool = row.get("is_burn_after_read");
        let burn_after_seconds: i32 = row.get("burn_after_seconds");
        let _seq: Option<i64> = row.get("seq");
        let _updated_at: Option<DateTime<Utc>> = row.get("updated_at");
        let visibility: Option<Value> = row.get("visibility");
        let read_by: Option<Value> = row.get("read_by");
        let operations: Option<Value> = row.get("operations");

        // 解析 receiver_ids
        let receiver_ids_vec: Vec<String> = receiver_ids
            .and_then(|v| from_value::<Vec<String>>(v).ok())
            .unwrap_or_default();

        // 解析 content (MessageContent protobuf)
        let content_proto = content.and_then(|bytes| ProstMessage::decode(&bytes[..]).ok());

        // 解析 extra JSONB
        let mut extra_map = HashMap::new();
        if let Some(extra_value) = extra {
            if let Ok(extra_obj) = from_value::<HashMap<String, Value>>(extra_value) {
                for (k, v) in extra_obj {
                    extra_map.insert(k, v.to_string().trim_matches('"').to_string());
                }
            }
        }

        // 从 extra 中提取租户信息
        let tenant = extra_map.get("tenant_id").map(|tenant_id| {
            let mut labels = HashMap::new();
            if let Some(labels_str) = extra_map.get("labels") {
                if let Ok(labels_obj) = serde_json::from_str::<HashMap<String, String>>(labels_str)
                {
                    labels = labels_obj;
                }
            }
            let mut tenant_attributes = HashMap::new();
            if let Some(attrs_str) = extra_map.get("tenant_attributes") {
                if let Ok(attrs_obj) = serde_json::from_str::<HashMap<String, String>>(attrs_str) {
                    tenant_attributes = attrs_obj;
                }
            }
            flare_proto::common::TenantContext {
                tenant_id: tenant_id.clone(),
                business_type: extra_map.get("business_type").cloned().unwrap_or_default(),
                environment: extra_map.get("environment").cloned().unwrap_or_default(),
                organization_id: extra_map
                    .get("organization_id")
                    .cloned()
                    .unwrap_or_default(),
                labels,
                attributes: tenant_attributes,
            }
        });

        // 从 extra 中提取其他字段
        // receiver_id 已废弃，通过 session_id 确定接收者
        let _session_type = extra_map.get("session_type").cloned().unwrap_or_default();
        let source_str = extra_map.get("sender_type").cloned().unwrap_or_default();
        let source = match source_str.as_str() {
            "user" => MessageSource::User as i32,
            "system" => MessageSource::System as i32,
            "bot" => MessageSource::Bot as i32,
            "admin" => MessageSource::Admin as i32,
            _ => MessageSource::Unspecified as i32,
        };

        // 解析 tags
        let tags: Vec<String> = extra_map
            .get("tags")
            .and_then(|tags_str| serde_json::from_str::<Vec<String>>(tags_str).ok())
            .unwrap_or_default();

        // 解析 attributes（从 extra 中提取，排除系统字段）
        let mut attributes = HashMap::new();
        for (k, v) in &extra_map {
            if !matches!(
                k.as_str(),
                "tenant_id"
                    | "business_id"
                    | "receiver_id"
                    | "session_type"
                    | "sender_type"
                    | "tags"
                    | "seq"
            ) {
                attributes.insert(k.clone(), v.clone());
            }
        }

        // 解析 visibility
        let mut visibility_map = HashMap::new();
        if let Some(vis_value) = visibility {
            if let Ok(vis_obj) = from_value::<HashMap<String, i32>>(vis_value) {
                for (user_id, status) in vis_obj {
                    visibility_map.insert(user_id, status);
                }
            }
        }

        // 解析 read_by
        let read_by_vec: Vec<MessageReadRecord> = read_by
            .and_then(|v| {
                // 尝试解析为 Vec<MessageReadRecord>
                // 注意：需要从 JSON 转换为 protobuf 结构
                from_value::<Vec<serde_json::Value>>(v)
                    .ok()
                    .and_then(|records| {
                        let mut result = Vec::new();
                        for record in records {
                            if let (Some(user_id), read_at_opt, burned_at_opt) = (
                                record.get("user_id").and_then(|v| v.as_str()),
                                record.get("read_at"),
                                record.get("burned_at"),
                            ) {
                                let read_at =
                                    read_at_opt.and_then(|v| v.as_object()).and_then(|obj| {
                                        let seconds = obj.get("seconds")?.as_i64()?;
                                        let nanos = obj.get("nanos")?.as_i64()?;
                                        Some(Timestamp {
                                            seconds,
                                            nanos: nanos as i32,
                                        })
                                    });
                                let burned_at =
                                    burned_at_opt.and_then(|v| v.as_object()).and_then(|obj| {
                                        let seconds = obj.get("seconds")?.as_i64()?;
                                        let nanos = obj.get("nanos")?.as_i64()?;
                                        Some(Timestamp {
                                            seconds,
                                            nanos: nanos as i32,
                                        })
                                    });
                                result.push(MessageReadRecord {
                                    user_id: user_id.to_string(),
                                    read_at,
                                    burned_at,
                                });
                            }
                        }
                        Some(result)
                    })
            })
            .unwrap_or_default();

        // 解析 operations（从 JSONB 转换为 protobuf）
        let operations_vec: Vec<MessageOperation> = operations
            .and_then(|v| {
                // 尝试解析为 Vec<serde_json::Value>
                from_value::<Vec<serde_json::Value>>(v)
                    .ok()
                    .and_then(|ops_json| {
                        let mut result = Vec::new();
                        for op_json in ops_json {
                            if let (
                                Some(operation_type),
                                Some(target_message_id),
                                Some(operator_id),
                                timestamp_opt,
                                show_notice,
                                notice_text,
                            ) = (
                                op_json
                                    .get("operation_type")
                                    .and_then(|v| v.as_i64())
                                    .map(|v| v as i32),
                                op_json.get("target_message_id").and_then(|v| v.as_str()),
                                op_json.get("operator_id").and_then(|v| v.as_str()),
                                op_json.get("timestamp"),
                                op_json
                                    .get("show_notice")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(true),
                                op_json
                                    .get("notice_text")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or(""),
                            ) {
                                let timestamp = timestamp_opt
                                    .and_then(|ts_obj| ts_obj.as_object())
                                    .and_then(|obj| {
                                        let seconds = obj.get("seconds")?.as_i64()?;
                                        let nanos = obj.get("nanos")?.as_i64()?;
                                        Some(Timestamp {
                                            seconds,
                                            nanos: nanos as i32,
                                        })
                                    });

                                // operation_data 是 oneof，需要根据实际类型解析
                                // 这里暂时设为 None，后续可以根据 operation_type 解析具体类型
                                let operation_data =
                                    op_json.get("operation_data").and_then(|_od| {
                                        // 简化：operation_data 的解析需要根据 operation_type 判断具体类型
                                        // 这里暂时返回 None，后续可以完善
                                        None
                                    });

                                // 解析 metadata（可选）
                                let mut metadata = HashMap::new();
                                if let Some(metadata_obj) =
                                    op_json.get("metadata").and_then(|v| v.as_object())
                                {
                                    for (k, v) in metadata_obj {
                                        if let Some(v_str) = v.as_str() {
                                            metadata.insert(k.clone(), v_str.to_string());
                                        }
                                    }
                                }

                                result.push(MessageOperation {
                                    operation_type,
                                    target_message_id: target_message_id.to_string(),
                                    operator_id: operator_id.to_string(),
                                    timestamp,
                                    show_notice,
                                    notice_text: notice_text.to_string(),
                                    target_user_id: op_json
                                        .get("target_user_id")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string())
                                        .unwrap_or_default(),
                                    operation_data,
                                    metadata,
                                    extensions: Vec::new(), // 添加必填字段 (prost_types::Any)
                                });
                            }
                        }
                        Some(result)
                    })
            })
            .unwrap_or_default();

        // 转换 message_type
        let message_type_enum = message_type
            .as_deref()
            .and_then(|s| match s {
                "text" => Some(MessageType::Text as i32),
                "image" => Some(MessageType::Image as i32),
                "video" => Some(MessageType::Video as i32),
                "audio" => Some(MessageType::Audio as i32),
                "file" => Some(MessageType::File as i32),
                "location" => Some(MessageType::Location as i32),
                "card" => Some(MessageType::Card as i32),
                "custom" => Some(MessageType::Custom as i32),
                "notification" => Some(MessageType::Notification as i32),
                "typing" => Some(MessageType::Typing as i32),
                "recall" => Some(MessageType::Recall as i32),
                "read" => Some(MessageType::Read as i32),
                "forward" => Some(MessageType::Forward as i32),
                _ => None,
            })
            .unwrap_or(MessageType::Unspecified as i32);

        // 转换 content_type
        let content_type_enum = content_type
            .as_deref()
            .and_then(|s| match s {
                "text/plain" => Some(ContentType::PlainText as i32),
                "text/html" => Some(ContentType::Html as i32),
                "text/markdown" => Some(ContentType::Markdown as i32),
                "application/json" => Some(ContentType::Json as i32),
                _ => None,
            })
            .unwrap_or(ContentType::Unspecified as i32);

        // 转换 status
        let status_enum = match status.as_str() {
            "created" => MessageStatus::Created as i32,
            "sent" => MessageStatus::Sent as i32,
            "delivered" => MessageStatus::Delivered as i32,
            "read" => MessageStatus::Read as i32,
            "failed" => MessageStatus::Failed as i32,
            "recalled" => MessageStatus::Recalled as i32,
            _ => MessageStatus::Unspecified as i32,
        };

        // 构建 Message
        Ok(Message {
            id,
            session_id,
            sender_id,
            receiver_id: String::new(), // 从数据库读取：receiver_id 可能为空（旧数据）
            channel_id: String::new(),  // 从数据库读取：channel_id 可能为空（旧数据）
            content: content_proto,
            timestamp: Some(datetime_to_timestamp(timestamp)),
            extra: extra_map,
            tenant,
            source,
            message_type: message_type_enum,
            content_type: content_type_enum,
            business_type,
            status: status_enum,
            is_recalled,
            recalled_at: recalled_at.map(|dt| datetime_to_timestamp(dt)),
            is_burn_after_read,
            burn_after_seconds,
            visibility: visibility_map,
            read_by: read_by_vec,
            tags,
            attributes,
            ..Default::default()
        })
    }
}

#[async_trait]
impl MessageStorage for PostgresMessageStorage {
    async fn store_message(&self, _message: &Message, _session_id: &str) -> Result<()> {
        // 读侧存储通常不需要实现 store_message
        // 但为了兼容性，可以提供一个空实现或委托给 Writer
        tracing::warn!(
            message_id = %_message.id,
            "store_message called on read-only storage, this should be handled by Storage Writer"
        );
        Ok(())
    }

    async fn query_messages(
        &self,
        session_id: &str,
        user_id: Option<&str>,
        start_time: Option<DateTime<Utc>>,
        end_time: Option<DateTime<Utc>>,
        limit: i32,
    ) -> Result<Vec<Message>> {
        let start_ts = start_time.unwrap_or_else(|| Utc::now() - chrono::Duration::days(7));
        let end_ts = end_time.unwrap_or(Utc::now());
        let limit = limit.min(1000).max(1); // 限制范围 1-1000

        // L2 缓存策略：先查 Redis，未命中再查 TimescaleDB
        if let Some(cache) = &self.cache {
            if let Ok(Some(cached_messages)) = cache
                .get_session_messages(session_id, start_ts, end_ts, limit)
                .await
            {
                tracing::debug!(
                    session_id = %session_id,
                    cached_count = cached_messages.len(),
                    "Cache hit: retrieved messages from Redis"
                );
                return Ok(cached_messages);
            }
        }

        // 缓存未命中，查询 TimescaleDB
        // 构建查询：利用 TimescaleDB 的时间分区裁剪优化
        // TimescaleDB 会自动裁剪不相关的分区，提高查询性能
        let mut query = sqlx::QueryBuilder::new(
            r#"
            SELECT 
                id, session_id, sender_id, receiver_ids, content, timestamp,
                extra, created_at, message_type, content_type, business_type,
                status, is_recalled, recalled_at, is_burn_after_read, burn_after_seconds,
                seq, updated_at, visibility, read_by, operations
            FROM messages
            WHERE session_id = 
            "#,
        );
        query.push_bind(session_id);
        // TimescaleDB 时间分区裁剪：使用 timestamp 范围查询，自动裁剪不相关的分区
        query.push(" AND timestamp >= ");
        query.push_bind(start_ts);
        query.push(" AND timestamp <= ");
        query.push_bind(end_ts);

        // 如果提供了 user_id，过滤已删除的消息
        if let Some(uid) = user_id {
            query.push(r#" AND (visibility->>$1 IS NULL OR (visibility->>$1)::int != 2)"#);
            query.push_bind(uid);
        }

        // 使用索引优化：session_id + timestamp DESC（复合索引）
        query.push(" ORDER BY timestamp DESC, seq DESC NULLS LAST");
        query.push(" LIMIT ");
        query.push_bind(limit);

        let rows = query
            .build()
            .fetch_all(&self.pool)
            .await
            .context("Failed to query messages")?;

        let mut messages = Vec::with_capacity(rows.len());
        for row in rows {
            messages.push(self.row_to_message(&row)?);
        }

        // 反转顺序，使最旧的消息在前（符合历史消息查询习惯）
        messages.reverse();

        // 回填缓存（异步，不阻塞）
        if let Some(cache) = &self.cache {
            let cache_clone = Arc::clone(cache);
            let messages_clone = messages.clone();
            let session_id_clone = session_id.to_string();
            tokio::spawn(async move {
                if let Err(e) = cache_clone
                    .cache_session_messages(&session_id_clone, start_ts, end_ts, &messages_clone)
                    .await
                {
                    tracing::warn!(
                        error = %e,
                        "Failed to cache messages to Redis (non-blocking)"
                    );
                }
            });
        }

        Ok(messages)
    }

    async fn query_messages_by_seq(
        &self,
        session_id: &str,
        user_id: Option<&str>,
        after_seq: i64,
        before_seq: Option<i64>,
        limit: i32,
    ) -> Result<Vec<Message>> {
        let limit = limit.min(1000).max(1);

        // 构建查询：基于 seq 查询（性能更好）
        let mut query = sqlx::QueryBuilder::new(
            r#"
            SELECT 
                id, session_id, sender_id, receiver_ids, content, timestamp,
                extra, created_at, message_type, content_type, business_type,
                status, is_recalled, recalled_at, is_burn_after_read, burn_after_seconds,
                seq, updated_at, visibility, read_by, operations
            FROM messages
            WHERE session_id = 
            "#,
        );
        query.push_bind(session_id);
        query.push(" AND seq > ");
        query.push_bind(after_seq);

        if let Some(before) = before_seq {
            query.push(" AND seq < ");
            query.push_bind(before);
        }

        // 如果提供了 user_id，过滤已删除的消息
        if let Some(uid) = user_id {
            query.push(r#" AND (visibility->>$1 IS NULL OR (visibility->>$1)::int != 2)"#);
            query.push_bind(uid);
        }

        query.push(" ORDER BY seq ASC");
        query.push(" LIMIT ");
        query.push_bind(limit);

        let rows = query
            .build()
            .fetch_all(&self.pool)
            .await
            .context("Failed to query messages by seq")?;

        let mut messages = Vec::with_capacity(rows.len());
        for row in rows {
            messages.push(self.row_to_message(&row)?);
        }

        Ok(messages)
    }

    async fn get_message(&self, message_id: &str) -> Result<Option<Message>> {
        // L2 缓存策略：先查 Redis，未命中再查 TimescaleDB
        // 注意：需要从 message_id 中提取 session_id，或通过查询获取
        // 简化实现：先查数据库获取 session_id，然后查缓存

        // 先尝试从数据库获取（包含 session_id）
        let row = sqlx::query(
            r#"
            SELECT 
                id, session_id, sender_id, receiver_ids, content, timestamp,
                extra, created_at, message_type, content_type, business_type,
                status, is_recalled, recalled_at, is_burn_after_read, burn_after_seconds,
                seq, updated_at, visibility, read_by, operations
            FROM messages
            WHERE id = $1
            LIMIT 1
            "#,
        )
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to get message")?;

        match row {
            Some(row) => {
                let message = self.row_to_message(&row)?;

                // 回填缓存（异步，不阻塞）
                if let Some(cache) = &self.cache {
                    let cache_clone = Arc::clone(cache);
                    let message_clone = message.clone();
                    tokio::spawn(async move {
                        if let Err(e) = cache_clone.cache_message(&message_clone).await {
                            tracing::warn!(
                                error = %e,
                                "Failed to cache message to Redis (non-blocking)"
                            );
                        }
                    });
                }

                Ok(Some(message))
            }
            None => Ok(None),
        }
    }

    async fn get_message_timestamp(&self, message_id: &str) -> Result<Option<DateTime<Utc>>> {
        // 直接查询消息的时间戳，避免加载完整的消息内容
        let row = sqlx::query(
            r#"
            SELECT timestamp
            FROM messages
            WHERE id = $1
            LIMIT 1
            "#,
        )
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to get message timestamp")?;

        match row {
            Some(row) => {
                let timestamp: DateTime<Utc> = row.get("timestamp");
                Ok(Some(timestamp))
            }
            None => Ok(None),
        }
    }

    async fn update_message(&self, message_id: &str, updates: MessageUpdate) -> Result<()> {
        // 使用 QueryBuilder 构建动态 UPDATE 语句
        let mut query = sqlx::QueryBuilder::new("UPDATE messages SET ");
        let mut has_updates = false;

        // 使用 separated 来添加逗号分隔的 SET 子句
        let mut separated = query.separated(", ");

        if let Some(is_recalled) = updates.is_recalled {
            separated.push("is_recalled = ");
            separated.push_bind(is_recalled);
            has_updates = true;
        }
        if let Some(recalled_at) = updates.recalled_at {
            separated.push("recalled_at = ");
            // timestamp_to_datetime 返回 Option<DateTime<Utc>>，需要 unwrap
            if let Some(dt) = timestamp_to_datetime(&recalled_at) {
                separated.push_bind(dt);
            } else {
                // 如果转换失败，使用 None
                separated.push_bind(Option::<DateTime<Utc>>::None);
            }
            has_updates = true;
        }
        if let Some(read_by) = updates.read_by {
            separated.push("read_by = ");
            // 将 protobuf 类型序列化为 JSON
            let read_by_json: Vec<serde_json::Value> = read_by
                .into_iter()
                .map(|record| {
                    serde_json::json!({
                        "user_id": record.user_id,
                        "read_at": record.read_at.map(|ts| {
                            serde_json::json!({
                                "seconds": ts.seconds,
                                "nanos": ts.nanos
                            })
                        }),
                        "burned_at": record.burned_at.map(|ts| {
                            serde_json::json!({
                                "seconds": ts.seconds,
                                "nanos": ts.nanos
                            })
                        }),
                    })
                })
                .collect();
            separated.push_bind(serde_json::to_value(&read_by_json)?);
            has_updates = true;
        }
        if let Some(operations) = updates.operations {
            separated.push("operations = ");
            // 将 protobuf 类型序列化为 JSON
            let operations_json: Vec<serde_json::Value> = operations
                .into_iter()
                .map(|op| {
                    serde_json::json!({
                        "operation_type": op.operation_type,
                        "operator_id": op.operator_id,
                        "target_message_id": op.target_message_id,
                        "timestamp": op.timestamp.map(|ts| {
                            serde_json::json!({
                                "seconds": ts.seconds,
                                "nanos": ts.nanos
                            })
                        }),
                        "show_notice": op.show_notice,
                        "notice_text": op.notice_text,
                        "target_user_id": op.target_user_id,
                        "metadata": op.metadata, // 序列化 metadata HashMap
                        "operation_data": if op.operation_data.is_some() {
                            // 简化：operation_data 是 oneof，需要根据实际类型序列化
                            // 暂时序列化为 null，后续可以完善
                            serde_json::Value::Null
                        } else {
                            serde_json::Value::Null
                        },
                    })
                })
                .collect();
            separated.push_bind(serde_json::to_value(&operations_json)?);
            has_updates = true;
        }
        if let Some(visibility) = updates.visibility {
            // 合并 visibility JSONB
            separated.push(r#"visibility = COALESCE(visibility, '{}'::jsonb) || "#);
            let vis_map: HashMap<String, i32> =
                visibility.into_iter().map(|(k, v)| (k, v as i32)).collect();
            separated.push_bind(serde_json::to_value(&vis_map)?);
            separated.push("::jsonb");
            has_updates = true;
        }
        if let Some(attributes) = updates.attributes {
            // 更新 extra 中的 attributes（需要合并到 extra JSONB）
            separated.push(r#"extra = COALESCE(extra, '{}'::jsonb) || "#);
            let attrs_json: HashMap<String, Value> = attributes
                .into_iter()
                .map(|(k, v)| (k, Value::String(v)))
                .collect();
            separated.push_bind(serde_json::to_value(&attrs_json)?);
            separated.push("::jsonb");
            has_updates = true;
        }
        if let Some(tags) = updates.tags {
            // 更新 extra 中的 tags
            separated
                .push(r#"extra = COALESCE(extra, '{}'::jsonb) || jsonb_build_object('tags', "#);
            separated.push_bind(serde_json::to_value(&tags)?);
            separated.push(")");
            has_updates = true;
        }
        if let Some(status) = updates.status {
            separated.push("status = ");
            // status 在数据库中存储为枚举字符串
            let status_str = MessageStatus::try_from(status)
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
            separated.push_bind(status_str);
            has_updates = true;
        }
        if let Some(reactions) = updates.reactions {
            separated.push("reactions = ");
            // 将 Reaction 列表序列化为 JSONB
            let reactions_json: Vec<serde_json::Value> = reactions
                .into_iter()
                .map(|reaction| {
                    serde_json::json!({
                        "emoji": reaction.emoji,
                        "user_ids": reaction.user_ids,
                        "count": reaction.count,
                        "last_updated": reaction.last_updated.map(|ts| {
                            serde_json::json!({
                                "seconds": ts.seconds,
                                "nanos": ts.nanos
                            })
                        }),
                        "created_at": reaction.created_at.map(|ts| {
                            serde_json::json!({
                                "seconds": ts.seconds,
                                "nanos": ts.nanos
                            })
                        }),
                    })
                })
                .collect();
            separated.push_bind(serde_json::to_value(&reactions_json)?);
            has_updates = true;
        }

        if !has_updates {
            return Ok(()); // 没有需要更新的字段
        }

        // 添加 updated_at
        separated.push("updated_at = CURRENT_TIMESTAMP");

        // 添加 WHERE 子句
        query.push(" WHERE id = ");
        query.push_bind(message_id);

        query
            .build()
            .execute(&self.pool)
            .await
            .context("Failed to update message")?;

        // 更新后清除缓存
        // 注意：需要 session_id 才能清除缓存，但这里只有 message_id
        // 实际生产环境可以维护 message_id -> session_id 的映射，或通过查询获取
        // 这里暂时不实现缓存失效，因为需要额外的查询开销
        if self.cache.is_some() {
            tracing::debug!(
                message_id = %message_id,
                "Message updated, cache invalidation skipped (requires session_id query)"
            );
        }

        Ok(())
    }

    async fn batch_update_visibility(
        &self,
        message_ids: &[String],
        user_id: &str,
        visibility: VisibilityStatus,
    ) -> Result<usize> {
        if message_ids.is_empty() {
            return Ok(0);
        }

        // 使用 JSONB 更新 visibility 字段
        let vis_value = visibility as i32;
        let vis_json = serde_json::json!({ user_id: vis_value });

        let result = sqlx::query(
            r#"
            UPDATE messages
            SET 
                visibility = COALESCE(visibility, '{}'::jsonb) || $1::jsonb,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = ANY($2)
            "#,
        )
        .bind(serde_json::to_value(&vis_json)?)
        .bind(message_ids)
        .execute(&self.pool)
        .await
        .context("Failed to batch update visibility")?;

        Ok(result.rows_affected() as usize)
    }

    async fn count_messages(
        &self,
        session_id: &str,
        user_id: Option<&str>,
        start_time: Option<DateTime<Utc>>,
        end_time: Option<DateTime<Utc>>,
    ) -> Result<i64> {
        let start_ts = start_time.unwrap_or_else(|| Utc::now() - chrono::Duration::days(7));
        let end_ts = end_time.unwrap_or(Utc::now());

        let mut query =
            sqlx::QueryBuilder::new("SELECT COUNT(*) FROM messages WHERE session_id = ");
        query.push_bind(session_id);
        query.push(" AND timestamp >= ");
        query.push_bind(start_ts);
        query.push(" AND timestamp <= ");
        query.push_bind(end_ts);

        if let Some(uid) = user_id {
            query.push(r#" AND (visibility->>$1 IS NULL OR (visibility->>$1)::int != 2)"#);
            query.push_bind(uid);
        }

        let count: i64 = query
            .build()
            .fetch_one(&self.pool)
            .await
            .and_then(|row| Ok(row.get::<i64, _>(0)))
            .context("Failed to count messages")?;

        Ok(count)
    }

    async fn search_messages(
        &self,
        filters: &[flare_proto::common::FilterExpression],
        start_time: Option<DateTime<Utc>>,
        end_time: Option<DateTime<Utc>>,
        limit: i32,
    ) -> Result<Vec<Message>> {
        let start_ts = start_time.unwrap_or_else(|| Utc::now() - chrono::Duration::days(7));
        let end_ts = end_time.unwrap_or(Utc::now());
        let limit = limit.min(1000).max(1);

        let mut query = sqlx::QueryBuilder::new(
            r#"
            SELECT 
                id, session_id, sender_id, receiver_ids, content, timestamp,
                extra, created_at, message_type, content_type, business_type,
                status, is_recalled, recalled_at, is_burn_after_read, burn_after_seconds,
                seq, updated_at, visibility, read_by, operations
            FROM messages
            WHERE timestamp >= 
            "#,
        );
        query.push_bind(start_ts);
        query.push(" AND timestamp <= ");
        query.push_bind(end_ts);

        // 应用过滤器
        for filter in filters {
            if filter.field.is_empty() || filter.values.is_empty() {
                continue;
            }

            match filter.field.as_str() {
                "session_id" => {
                    query.push(" AND session_id = ");
                    query.push_bind(&filter.values[0]);
                }
                "sender_id" => {
                    query.push(" AND sender_id = ");
                    query.push_bind(&filter.values[0]);
                }
                "message_type" => {
                    query.push(" AND message_type = ");
                    query.push_bind(&filter.values[0]);
                }
                "status" => {
                    query.push(" AND status = ");
                    query.push_bind(&filter.values[0]);
                }
                "is_recalled" => {
                    query.push(" AND is_recalled = ");
                    query.push_bind(filter.values[0].parse::<bool>().unwrap_or(false));
                }
                _ => {
                    // 其他字段暂不支持，忽略
                }
            }
        }

        query.push(" ORDER BY timestamp DESC, seq DESC NULLS LAST");
        query.push(" LIMIT ");
        query.push_bind(limit);

        let rows = query
            .build()
            .fetch_all(&self.pool)
            .await
            .context("Failed to search messages")?;

        let mut messages = Vec::with_capacity(rows.len());
        for row in rows {
            messages.push(self.row_to_message(&row)?);
        }

        Ok(messages)
    }

    async fn update_message_attributes(
        &self,
        message_id: &str,
        attributes: HashMap<String, String>,
        tags: Vec<String>,
    ) -> Result<()> {
        // 更新 extra JSONB 中的 attributes 和 tags
        let mut extra_updates = serde_json::Map::new();

        // 添加 attributes
        for (k, v) in &attributes {
            extra_updates.insert(k.clone(), serde_json::Value::String(v.clone()));
        }

        // 添加 tags
        if !tags.is_empty() {
            extra_updates.insert(
                "tags".to_string(),
                serde_json::Value::Array(
                    tags.iter()
                        .map(|t| serde_json::Value::String(t.clone()))
                        .collect(),
                ),
            );
        }

        sqlx::query(
            r#"
            UPDATE messages
            SET 
                extra = COALESCE(extra, '{}'::jsonb) || $1::jsonb,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = $2
            "#,
        )
        .bind(serde_json::to_value(&extra_updates)?)
        .bind(message_id)
        .execute(&self.pool)
        .await
        .context("Failed to update message attributes")?;

        Ok(())
    }

    async fn list_all_tags(&self) -> Result<Vec<String>> {
        // 从 extra JSONB 中提取所有 tags
        let rows = sqlx::query(
            r#"
            SELECT DISTINCT jsonb_array_elements_text(extra->'tags') as tag
            FROM messages
            WHERE extra->'tags' IS NOT NULL
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to list tags")?;

        let mut tags = Vec::new();
        for row in rows {
            if let Ok(tag) = row.try_get::<String, _>("tag") {
                tags.push(tag);
            }
        }

        Ok(tags)
    }
}

#[async_trait]
impl VisibilityStorage for PostgresMessageStorage {
    async fn set_visibility(
        &self,
        message_id: &str,
        user_id: &str,
        _session_id: &str,
        visibility: VisibilityStatus,
    ) -> Result<()> {
        let vis_value = visibility as i32;
        let vis_json = serde_json::json!({ user_id: vis_value });

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
        .await
        .context("Failed to set visibility")?;

        Ok(())
    }

    async fn get_visibility(
        &self,
        message_id: &str,
        user_id: &str,
    ) -> Result<Option<VisibilityStatus>> {
        let row = sqlx::query(
            r#"
            SELECT visibility->$1 as vis_status
            FROM messages
            WHERE id = $2
            "#,
        )
        .bind(user_id)
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to get visibility")?;

        match row {
            Some(row) => {
                let vis_value: Option<i32> = row.get("vis_status");
                match vis_value {
                    Some(v) => Ok(Some(
                        VisibilityStatus::try_from(v)
                            .unwrap_or(VisibilityStatus::VisibilityVisible),
                    )),
                    None => Ok(Some(VisibilityStatus::VisibilityVisible)), // 默认可见
                }
            }
            None => Ok(None),
        }
    }

    async fn batch_set_visibility(
        &self,
        message_ids: &[String],
        user_id: &str,
        _session_id: &str,
        visibility: VisibilityStatus,
    ) -> Result<usize> {
        self.batch_update_visibility(message_ids, user_id, visibility)
            .await
    }

    async fn query_visible_message_ids(
        &self,
        user_id: &str,
        session_id: &str,
        visibility_status: VisibilityStatus,
    ) -> Result<Vec<String>> {
        let vis_value = visibility_status as i32;

        let rows = sqlx::query(
            r#"
            SELECT id
            FROM messages
            WHERE session_id = $1
            AND (visibility->$2)::int = $3
            "#,
        )
        .bind(session_id)
        .bind(user_id)
        .bind(vis_value)
        .fetch_all(&self.pool)
        .await
        .context("Failed to query visible message ids")?;

        let mut message_ids = Vec::new();
        for row in rows {
            message_ids.push(row.get::<String, _>("id"));
        }

        Ok(message_ids)
    }
}
