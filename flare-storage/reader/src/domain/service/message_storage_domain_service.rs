//! 消息存储领域服务 - 包含所有业务逻辑实现

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use anyhow::{Result, anyhow};
use chrono::{DateTime, Duration, TimeZone, Utc};
use flare_im_core::utils::{TimelineMetadata, extract_timeline_from_extra, timestamp_to_datetime, extract_seq_from_message};
use flare_proto::common::{Message, VisibilityStatus};
use prost_types::Timestamp;
use tracing::instrument;

use crate::domain::model::MessageUpdate;
use crate::domain::repository::{MessageStorage, VisibilityStorage};

/// 领域服务配置（值对象，不依赖基础设施层）
#[derive(Debug, Clone)]
pub struct MessageStorageDomainConfig {
    pub max_page_size: i32,
    pub default_range_seconds: i64,
}

/// 查询游标
struct QueryCursor {
    ingestion_ts: i64,
    message_id: String,
}

impl QueryCursor {
    fn from_raw(raw: Option<&str>) -> Option<Self> {
        let raw = raw?;
        let mut parts = raw.splitn(2, ':');
        let ts = parts.next()?.parse::<i64>().ok()?;
        let message_id = parts.next()?.to_string();
        Some(Self {
            ingestion_ts: ts,
            message_id,
        })
    }
}

/// 检索到的消息
struct RetrievedMessage {
    message: Message,
    timeline: TimelineMetadata,
}

impl RetrievedMessage {
    fn new(message: Message, timeline: TimelineMetadata) -> Self {
        Self { message, timeline }
    }
}

/// 查询消息结果
pub struct QueryMessagesResult {
    pub messages: Vec<Message>,
    pub next_cursor: String,
    pub has_more: bool,
    pub total_size: i64,
}

/// 消息存储领域服务 - 包含所有业务逻辑
pub struct MessageStorageDomainService {
    storage: Arc<dyn MessageStorage + Send + Sync>,
    visibility_storage: Option<Arc<dyn VisibilityStorage + Send + Sync>>,
    config: MessageStorageDomainConfig,
}

impl MessageStorageDomainService {
    pub fn new(
        storage: Arc<dyn MessageStorage + Send + Sync>,
        visibility_storage: Option<Arc<dyn VisibilityStorage + Send + Sync>>,
        config: MessageStorageDomainConfig,
    ) -> Self {
        Self {
            storage,
            visibility_storage,
            config,
        }
    }

    /// 查询消息列表（基于时间戳，向后兼容）
    #[instrument(skip(self), fields(session_id = %session_id))]
    pub async fn query_messages(
        &self,
        session_id: &str,
        start_time: i64,
        end_time: i64,
        limit: i32,
        cursor: Option<&str>,
    ) -> Result<QueryMessagesResult> {
        if session_id.is_empty() {
            return Err(anyhow!("session_id is required"));
        }

        let limit = limit.clamp(1, self.config.max_page_size) as usize;
        let cursor = QueryCursor::from_raw(cursor);

        let end_ts = if end_time == 0 {
            Utc::now().timestamp()
        } else {
            end_time
        };
        let start_ts = if start_time == 0 {
            end_ts - self.config.default_range_seconds
        } else {
            start_time
        };

        let end_ts_ms = end_ts * 1_000;
        let start_ts_ms = start_ts * 1_000;

        // 计算总记录数
        let start_dt_for_count = Utc
            .timestamp_opt(start_ts, 0)
            .single()
            .unwrap_or_else(|| Utc::now() - Duration::seconds(self.config.default_range_seconds));
        let end_dt_for_count = Utc
            .timestamp_opt(end_ts, 0)
            .single()
            .unwrap_or_else(Utc::now);

        let total_size = self
            .storage
            .count_messages(session_id, None, Some(start_dt_for_count), Some(end_dt_for_count))
            .await
            .map_err(|e| anyhow!("Failed to count messages: {}", e))?;

        let mut seen = HashSet::new();
        if let Some(cursor) = &cursor {
            seen.insert(cursor.message_id.clone());
        }

        let mut aggregated = self
            .query_from_storage(
                session_id,
                start_ts_ms,
                end_ts_ms,
                cursor.as_ref(),
                limit,
                &mut seen,
            )
            .await?;

        aggregated.sort_by(|a, b| b.timeline.ingestion_ts.cmp(&a.timeline.ingestion_ts));
        aggregated.truncate(limit);

        let messages: Vec<Message> = aggregated.iter().map(|item| item.message.clone()).collect();
        let next_cursor = if messages.len() == limit {
            aggregated
                .last()
                .map(|last| format!("{}:{}", last.timeline.ingestion_ts, last.message.id))
                .unwrap_or_default()
        } else {
            String::new()
        };

        Ok(QueryMessagesResult {
            messages,
            next_cursor: next_cursor.clone(),
            has_more: !next_cursor.is_empty(),
            total_size,
        })
    }

    /// 基于 seq 查询消息（推荐，性能更好）
    ///
    /// # 参数
    /// * `session_id` - 会话ID
    /// * `user_id` - 用户ID（可选，用于过滤已删除消息）
    /// * `after_seq` - 查询 seq > after_seq 的消息（用于增量同步）
    /// * `before_seq` - 查询 seq < before_seq 的消息（可选，用于分页）
    /// * `limit` - 返回消息数量限制
    ///
    /// # 返回
    /// * `Ok(QueryMessagesResult)` - 消息列表（按 seq 升序排序）
    #[instrument(skip(self), fields(session_id = %session_id, after_seq, before_seq = ?before_seq))]
    pub async fn query_messages_by_seq(
        &self,
        session_id: &str,
        user_id: Option<&str>,
        after_seq: i64,
        before_seq: Option<i64>,
        limit: i32,
    ) -> Result<QueryMessagesResult> {
        if session_id.is_empty() {
            return Err(anyhow!("session_id is required"));
        }

        let limit = limit.clamp(1, self.config.max_page_size) as usize;

        // 使用基于 seq 的查询
        let messages = self
            .storage
            .query_messages_by_seq(session_id, user_id, after_seq, before_seq, limit as i32)
            .await
            .map_err(|e| anyhow!("Failed to query messages by seq: {}", e))?;

        // 构建 next_cursor（基于最后一个消息的 seq）
        let next_cursor = if messages.len() == limit {
            messages
                .last()
                .and_then(|msg| {
                    // 从 extra 字段提取 seq（使用工具函数）
                    extract_seq_from_message(msg)
                        .map(|seq| format!("seq:{}:{}", seq, msg.id))
                })
                .unwrap_or_default()
        } else {
            String::new()
        };

        // 计算总记录数（简化实现：使用消息数量）
        let total_size = messages.len() as i64;

        Ok(QueryMessagesResult {
            messages,
            next_cursor: next_cursor.clone(),
            has_more: !next_cursor.is_empty(),
            total_size,
        })
    }

    async fn query_from_storage(
        &self,
        session_id: &str,
        start_ts_ms: i64,
        end_ts_ms: i64,
        cursor: Option<&QueryCursor>,
        limit: usize,
        seen: &mut HashSet<String>,
    ) -> Result<Vec<RetrievedMessage>> {
        let start_dt = Utc
            .timestamp_millis_opt(start_ts_ms)
            .single()
            .unwrap_or_else(|| Utc::now() - Duration::days(30));
        let mut end_dt = Utc
            .timestamp_millis_opt(end_ts_ms)
            .single()
            .unwrap_or_else(Utc::now);

        if let Some(cursor) = cursor {
            if cursor.ingestion_ts <= start_ts_ms {
                return Ok(Vec::new());
            }
            end_dt = Utc
                .timestamp_millis_opt(cursor.ingestion_ts - 1)
                .single()
                .unwrap_or(end_dt);
        }

        if end_dt < start_dt {
            return Ok(Vec::new());
        }

        let messages = self
            .storage
            .query_messages(session_id, None, Some(start_dt), Some(end_dt), limit as i32)
            .await
            .map_err(|err| anyhow!(err.to_string()))?;

        let mut results = Vec::new();
        for message in messages {
            if !seen.insert(message.id.clone()) {
                continue;
            }

            let ingestion_hint = message
                .timestamp
                .as_ref()
                .and_then(timestamp_to_datetime)
                .map(|dt| dt.timestamp_millis())
                .unwrap_or_else(|| Utc::now().timestamp_millis());

            let timeline = extract_timeline_from_extra(&message.extra, ingestion_hint);
            results.push(RetrievedMessage::new(message, timeline));
            if results.len() >= limit {
                break;
            }
        }

        Ok(results)
    }

    /// 获取单条消息
    #[instrument(skip(self), fields(message_id = %message_id))]
    pub async fn get_message(&self, message_id: &str) -> Result<Option<Message>> {
        self.storage
            .get_message(message_id)
            .await
            .map_err(|e| anyhow!("Failed to get message: {}", e))
    }

    /// 搜索消息
    #[instrument(skip(self))]
    pub async fn search_messages(
        &self,
        filters: &[flare_proto::common::FilterExpression],
        start_time: Option<DateTime<Utc>>,
        end_time: Option<DateTime<Utc>>,
        limit: i32,
    ) -> Result<Vec<Message>> {
        let limit = limit.clamp(1, self.config.max_page_size);
        self.storage
            .search_messages(filters, start_time, end_time, limit)
            .await
            .map_err(|e| anyhow!("Failed to search messages: {}", e))
    }

    /// 列出所有标签
    #[instrument(skip(self))]
    pub async fn list_all_tags(&self) -> Result<Vec<String>> {
        self.storage
            .list_all_tags()
            .await
            .map_err(|e| anyhow!("Failed to list tags: {}", e))
    }

    /// 删除消息（批量）
    #[instrument(skip(self), fields(message_count = message_ids.len()))]
    pub async fn delete_messages(&self, message_ids: &[String]) -> Result<usize> {
        if message_ids.is_empty() {
            return Ok(0);
        }

        let mut deleted_count = 0;
        for message_id in message_ids {
            match self
                .storage
                .batch_update_visibility(
                    &[message_id.clone()],
                    "", // 系统删除，不需要 user_id
                    VisibilityStatus::Deleted,
                )
                .await
            {
                Ok(count) => deleted_count += count,
                Err(err) => {
                    tracing::warn!(error = %err, message_id = %message_id, "Failed to delete message");
                }
            }
        }

        Ok(deleted_count)
    }

    /// 撤回消息
    #[instrument(skip(self), fields(message_id = %message_id))]
    pub async fn recall_message(
        &self,
        message_id: &str,
        recall_time_limit_seconds: i64,
    ) -> Result<Option<Timestamp>> {
        if message_id.is_empty() {
            return Err(anyhow!("message_id is required"));
        }

        // 检查消息是否存在
        let message = match self.get_message(message_id).await? {
            Some(msg) => msg,
            None => return Err(anyhow!("message not found")),
        };

        // 检查撤回时间限制
        let message_timestamp = message
            .timestamp
            .as_ref()
            .map(|ts| ts.seconds)
            .unwrap_or(0);
        let now = Utc::now().timestamp();
        let elapsed = now - message_timestamp;

        if elapsed > recall_time_limit_seconds {
            return Err(anyhow!(
                "Message is too old to recall (limit: {}s, elapsed: {}s)",
                recall_time_limit_seconds,
                elapsed
            ));
        }

        // 执行撤回
        let recalled_at = Utc::now();
        let recalled_timestamp = Timestamp {
            seconds: recalled_at.timestamp(),
            nanos: recalled_at.timestamp_subsec_nanos() as i32,
        };

        let update = MessageUpdate {
            is_recalled: Some(true),
            recalled_at: Some(recalled_timestamp.clone()),
            visibility: None,
            read_by: None,
            operations: None,
            attributes: None,
            tags: None,
        };

        self.storage
            .update_message(message_id, update)
            .await
            .map_err(|e| anyhow!("Failed to recall message: {}", e))?;

        Ok(Some(recalled_timestamp))
    }

    /// 标记消息已读
    #[instrument(skip(self), fields(message_id = %message_id, user_id = %user_id))]
    pub async fn mark_message_read(
        &self,
        message_id: &str,
        user_id: &str,
    ) -> Result<(Timestamp, Option<Timestamp>)> {
        if message_id.is_empty() {
            return Err(anyhow!("message_id is required"));
        }
        if user_id.is_empty() {
            return Err(anyhow!("user_id is required"));
        }

        // 获取消息
        let message = match self.get_message(message_id).await? {
            Some(msg) => msg,
            None => return Err(anyhow!("message not found")),
        };

        let now = Utc::now();
        let read_timestamp = Timestamp {
            seconds: now.timestamp(),
            nanos: now.timestamp_subsec_nanos() as i32,
        };

        // 检查是否是阅后即焚消息
        let burned_at = if message.is_burn_after_read {
            let burn_seconds = message.burn_after_seconds as i64;
            Some(Timestamp {
                seconds: now.timestamp() + burn_seconds,
                nanos: now.timestamp_subsec_nanos() as i32,
            })
        } else {
            None
        };

        // 更新已读记录
        let mut read_by = message.read_by.clone();
        let read_record = flare_proto::common::MessageReadRecord {
            user_id: user_id.to_string(),
            read_at: Some(read_timestamp.clone()),
            burned_at: burned_at.clone(),
        };

        // 检查是否已存在该用户的已读记录
        if let Some(existing) = read_by.iter_mut().find(|r| r.user_id == user_id) {
            existing.read_at = Some(read_timestamp.clone());
            existing.burned_at = burned_at.clone();
        } else {
            read_by.push(read_record);
        }

        let update = MessageUpdate {
            is_recalled: None,
            recalled_at: None,
            visibility: None,
            read_by: Some(read_by),
            operations: None,
            attributes: None,
            tags: None,
        };

        self.storage
            .update_message(message_id, update)
            .await
            .map_err(|e| anyhow!("Failed to mark message as read: {}", e))?;

        Ok((read_timestamp, burned_at))
    }

    /// 为用户删除消息（软删除）
    #[instrument(skip(self), fields(message_id = %message_id, user_id = %user_id))]
    pub async fn delete_message_for_user(
        &self,
        message_id: &str,
        user_id: &str,
        permanent: bool,
    ) -> Result<usize> {
        if message_id.is_empty() {
            return Err(anyhow!("message_id is required"));
        }
        if user_id.is_empty() {
            return Err(anyhow!("user_id is required"));
        }

        // 检查消息是否存在
        let message = match self.get_message(message_id).await? {
            Some(msg) => msg,
            None => return Err(anyhow!("message not found")),
        };

        // 软删除：更新 visibility
        let visibility = if permanent {
            VisibilityStatus::Deleted
        } else {
            VisibilityStatus::Hidden
        };

        if let Some(visibility_storage) = &self.visibility_storage {
            visibility_storage
                .batch_set_visibility(
                    &[message_id.to_string()],
                    user_id,
                    &message.session_id,
                    visibility,
                )
                .await
                .map_err(|e| anyhow!("Failed to delete message for user: {}", e))
        } else {
            self.storage
                .batch_update_visibility(&[message_id.to_string()], user_id, visibility)
                .await
                .map_err(|e| anyhow!("Failed to delete message for user: {}", e))
        }
    }

    /// 设置消息属性
    #[instrument(skip(self), fields(message_id = %message_id))]
    pub async fn set_message_attributes(
        &self,
        message_id: &str,
        attributes: HashMap<String, String>,
        tags: Vec<String>,
    ) -> Result<()> {
        // 默认行为：仅更新属性与标签
        self.storage
            .update_message_attributes(message_id, attributes, tags)
            .await
            .map_err(|e| anyhow!("Failed to set message attributes: {}", e))
    }

    /// 追加一条操作记录并同时更新属性与标签
    #[instrument(skip(self), fields(message_id = %message_id, operation_type = %operation.operation_type))]
    pub async fn append_operation_and_attributes(
        &self,
        message_id: &str,
        operation: flare_proto::common::MessageOperation,
        attributes: HashMap<String, String>,
        tags: Vec<String>,
    ) -> Result<()> {
        // 读取当前消息以获取已有操作记录
        let current = self.storage
            .get_message(message_id)
            .await
            .map_err(|e| anyhow!("Failed to get message for operation append: {}", e))?;

        let mut operations = current
            .map(|m| m.operations)
            .unwrap_or_default();
        operations.push(operation);

        let updates = crate::domain::model::MessageUpdate {
            is_recalled: None,
            recalled_at: None,
            visibility: None,
            read_by: None,
            operations: Some(operations),
            attributes: Some(attributes),
            tags: Some(tags),
        };

        self.storage
            .update_message(message_id, updates)
            .await
            .map_err(|e| anyhow!("Failed to update message with operation: {}", e))
    }

    /// 清理会话
    #[instrument(skip(self), fields(session_id = %session_id))]
    pub async fn clear_session(
        &self,
        session_id: &str,
        user_id: &str,
        clear_before_time: Option<DateTime<Utc>>,
    ) -> Result<usize> {
        if session_id.is_empty() {
            return Err(anyhow!("session_id is required"));
        }

        // 查询需要清理的消息
        let messages = self
            .storage
            .query_messages(session_id, Some(user_id), None, clear_before_time, 10000)
            .await
            .map_err(|e| anyhow!("Failed to query messages: {}", e))?;

        let cleared_count = messages.len();

        // 批量更新 visibility 为 DELETED
        let message_ids: Vec<String> = messages.iter().map(|m| m.id.clone()).collect();
        if !message_ids.is_empty() {
            self.storage
                .batch_update_visibility(&message_ids, user_id, VisibilityStatus::Deleted)
                .await
                .map_err(|e| anyhow!("Failed to clear session: {}", e))?;
        }

        Ok(cleared_count)
    }
}
