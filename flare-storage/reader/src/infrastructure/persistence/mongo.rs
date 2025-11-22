//! 内存版读侧存储实现
//!
//! 为了简化依赖并修复编译问题，这里提供一个纯内存的存储实现。
//! 该实现仅用于开发和 CI 环境，生产部署需替换为真实的数据存储。

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use chrono::{DateTime, Utc};
use flare_proto::common::{Message, VisibilityStatus};
use tokio::sync::RwLock;
use tracing::warn;

use crate::domain::model::MessageUpdate;
use crate::domain::repository::{MessageStorage, VisibilityStorage};

#[derive(Default, Clone)]
struct StoredMessage {
    session_id: String,
    message: Message,
    updated_at: i64,
}

// 注意：SessionInfo 类型不存在于 storage.proto 中
// 会话管理应该由 SessionService 处理，这里暂时注释掉
// #[derive(Default, Clone)]
// struct StoredSession {
//     session: SessionInfo,
//     updated_at: i64,
// }

#[derive(Default)]
pub struct MongoMessageStorage {
    messages: Arc<RwLock<HashMap<String, StoredMessage>>>,
    session_index: Arc<RwLock<HashMap<String, Vec<String>>>>,
    // sessions: Arc<RwLock<HashMap<String, StoredSession>>>, // 会话管理已移除
    visibility: Arc<RwLock<HashMap<(String, String), VisibilityStatus>>>,
}

impl MongoMessageStorage {
    pub async fn new(_uri: &str, _database: &str) -> Result<Self> {
        Ok(Self::default())
    }

    fn message_timestamp(message: &Message) -> i64 {
        message
            .timestamp
            .as_ref()
            .map(|ts| (ts.seconds * 1_000) + (ts.nanos as i64 / 1_000_000))
            .unwrap_or_else(|| Utc::now().timestamp_millis())
    }
}

#[async_trait::async_trait]
impl MessageStorage for MongoMessageStorage {
    async fn store_message(
        &self,
        message: &Message,
        session_id: &str,
    ) -> Result<()> {
        let mut stored = message.clone();
        if stored.session_id.is_empty() {
            stored.session_id = session_id.to_string();
        }

        let mut messages = self.messages.write().await;
        let mut session_index = self.session_index.write().await;

        let timestamp = Self::message_timestamp(&stored);

        messages.insert(
            stored.id.clone(),
            StoredMessage {
                session_id: session_id.to_string(),
                message: stored.clone(),
                updated_at: timestamp,
            },
        );
        session_index
            .entry(session_id.to_string())
            .or_default()
            .push(stored.id.clone());

        Ok(())
    }

    async fn query_messages(
        &self,
        session_id: &str,
        _user_id: Option<&str>,
        start_time: Option<DateTime<Utc>>,
        end_time: Option<DateTime<Utc>>,
        limit: i32,
    ) -> Result<Vec<Message>> {
        let session_index = self.session_index.read().await;
        let messages = self.messages.read().await;

        let ids = session_index.get(session_id).cloned().unwrap_or_default();

        let start_ms = start_time
            .map(|ts| ts.timestamp_millis())
            .unwrap_or(i64::MIN);
        let end_ms = end_time.map(|ts| ts.timestamp_millis()).unwrap_or(i64::MAX);

        let mut collected = Vec::new();
        for message_id in ids.iter().rev() {
            if let Some(record) = messages.get(message_id) {
                if record.session_id != session_id {
                    continue;
                }
                if record.updated_at < start_ms || record.updated_at > end_ms {
                    continue;
                }
                collected.push(record.message.clone());
                if collected.len() as i32 >= limit {
                    break;
                }
            }
        }

        Ok(collected)
    }

    async fn get_message(
        &self,
        message_id: &str,
    ) -> Result<Option<Message>> {
        let messages = self.messages.read().await;
        Ok(messages
            .get(message_id)
            .map(|record| record.message.clone()))
    }

    async fn update_message(
        &self,
        message_id: &str,
        updates: MessageUpdate,
    ) -> Result<()> {
        let mut messages = self.messages.write().await;
        if let Some(record) = messages.get_mut(message_id) {
            let message = &mut record.message;

            if let Some(is_recalled) = updates.is_recalled {
                message.is_recalled = is_recalled;
            }
            if let Some(recalled_at) = updates.recalled_at {
                message.recalled_at = Some(recalled_at);
            }
            // recall_reason字段在proto中不存在，已移除
            // 如果需要记录撤回原因，可以存储在extra字段中
            if let Some(read_by) = updates.read_by {
                message.read_by = read_by;
            }
            if let Some(operations) = updates.operations {
                message.operations = operations;
            }
            if let Some(visibility) = updates.visibility {
                for (user_id, status) in visibility {
                    message.visibility.insert(user_id.clone(), status as i32);
                    self.visibility
                        .write()
                        .await
                        .insert((message_id.to_string(), user_id), status);
                }
            }
            if let Some(attributes) = updates.attributes {
                message.attributes.extend(attributes);
            }
            if let Some(tags) = updates.tags {
                message.tags = tags;
            }
            record.updated_at = Utc::now().timestamp_millis();
        }

        Ok(())
    }

    async fn batch_update_visibility(
        &self,
        message_ids: &[String],
        user_id: &str,
        visibility: VisibilityStatus,
    ) -> Result<usize> {
        let mut updated = 0usize;
        let mut vis_map = self.visibility.write().await;
        for message_id in message_ids {
            vis_map.insert((message_id.clone(), user_id.to_string()), visibility);
            updated += 1;
        }
        Ok(updated)
    }

    async fn count_messages(
        &self,
        session_id: &str,
        _user_id: Option<&str>,
        start_time: Option<DateTime<Utc>>,
        end_time: Option<DateTime<Utc>>,
    ) -> Result<i64> {
        let session_index = self.session_index.read().await;
        let messages = self.messages.read().await;

        let ids = session_index.get(session_id).cloned().unwrap_or_default();

        let start_ms = start_time
            .map(|ts| ts.timestamp_millis())
            .unwrap_or(i64::MIN);
        let end_ms = end_time.map(|ts| ts.timestamp_millis()).unwrap_or(i64::MAX);

        let mut count = 0i64;
        for message_id in &ids {
            if let Some(record) = messages.get(message_id) {
                if record.session_id != session_id {
                    continue;
                }
                if record.updated_at >= start_ms && record.updated_at <= end_ms {
                    count += 1;
                }
            }
        }

        Ok(count)
    }

    async fn search_messages(
        &self,
        filters: &[flare_proto::common::FilterExpression],
        start_time: Option<DateTime<Utc>>,
        end_time: Option<DateTime<Utc>>,
        limit: i32,
    ) -> Result<Vec<Message>> {
        let messages = self.messages.read().await;

        let start_ms = start_time
            .map(|ts| ts.timestamp_millis())
            .unwrap_or(i64::MIN);
        let end_ms = end_time.map(|ts| ts.timestamp_millis()).unwrap_or(i64::MAX);

        let mut results = Vec::new();
        for (_message_id, record) in messages.iter() {
            // 时间范围过滤
            if record.updated_at < start_ms || record.updated_at > end_ms {
                continue;
            }

            // 应用过滤器（简化实现：只支持基本的字段过滤和 EQUAL 操作）
            let mut matched = true;
            for filter in filters {
                if !filter.field.is_empty() && !filter.values.is_empty() {
                    let value = &filter.values[0]; // 简化实现：只使用第一个值
                    match filter.field.as_str() {
                        "sender_id" => {
                            if record.message.sender_id != *value {
                                matched = false;
                                break;
                            }
                        }
                        "session_id" => {
                            if record.session_id != *value {
                                matched = false;
                                break;
                            }
                        }
                        "message_type" => {
                            if let Ok(msg_type) = value.parse::<i32>() {
                                if record.message.message_type() as i32 != msg_type {
                                    matched = false;
                                    break;
                                }
                            }
                        }
                        _ => {
                            // 其他字段暂不支持，忽略
                        }
                    }
                }
                if !matched {
                    break;
                }
            }

            if matched {
                results.push(record.message.clone());
                if results.len() as i32 >= limit {
                    break;
                }
            }
        }

        Ok(results)
    }

    async fn update_message_attributes(
        &self,
        message_id: &str,
        attributes: std::collections::HashMap<String, String>,
        tags: Vec<String>,
    ) -> Result<()> {
        let mut messages = self.messages.write().await;
        if let Some(record) = messages.get_mut(message_id) {
            let message = &mut record.message;
            message.attributes.extend(attributes);
            message.tags = tags;
            record.updated_at = Utc::now().timestamp_millis();
        }
        Ok(())
    }

    async fn list_all_tags(
        &self,
    ) -> Result<Vec<String>> {
        let messages = self.messages.read().await;
        let mut tag_set = std::collections::HashSet::new();

        for (_message_id, record) in messages.iter() {
            for tag in &record.message.tags {
                tag_set.insert(tag.clone());
            }
        }

        Ok(tag_set.into_iter().collect())
    }

    async fn query_messages_by_seq(
        &self,
        session_id: &str,
        _user_id: Option<&str>,
        after_seq: i64,
        before_seq: Option<i64>,
        limit: i32,
    ) -> Result<Vec<Message>> {
        let session_index = self.session_index.read().await;
        let messages = self.messages.read().await;

        let ids = session_index.get(session_id).cloned().unwrap_or_default();

        // 从消息的 extra 字段提取 seq（使用工具函数）
        fn extract_seq(message: &Message) -> i64 {
            flare_im_core::utils::extract_seq_from_message(message).unwrap_or(0)
        }

        let mut collected = Vec::new();
        for message_id in &ids {
            if let Some(record) = messages.get(message_id) {
                if record.session_id != session_id {
                    continue;
                }
                
                let seq = extract_seq(&record.message);
                
                // 过滤 seq 范围
                if seq <= after_seq {
                    continue;
                }
                if let Some(before) = before_seq {
                    if seq >= before {
                        continue;
                    }
                }
                
                collected.push((seq, record.message.clone()));
                if collected.len() as i32 >= limit {
                    break;
                }
            }
        }

        // 按 seq 升序排序
        collected.sort_by_key(|(seq, _)| *seq);
        Ok(collected.into_iter().map(|(_, msg)| msg).collect())
    }
}

// 注意：会话管理不属于 StorageService，已移除
// 会话管理应该由 SessionService 处理

#[async_trait::async_trait]
impl VisibilityStorage for MongoMessageStorage {
    async fn set_visibility(
        &self,
        message_id: &str,
        user_id: &str,
        _session_id: &str,
        visibility: VisibilityStatus,
    ) -> Result<()> {
        self.visibility
            .write()
            .await
            .insert((message_id.to_string(), user_id.to_string()), visibility);
        Ok(())
    }

    async fn get_visibility(
        &self,
        message_id: &str,
        user_id: &str,
    ) -> Result<Option<VisibilityStatus>> {
        let vis_map = self.visibility.read().await;
        Ok(vis_map
            .get(&(message_id.to_string(), user_id.to_string()))
            .copied())
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
        let messages = self.messages.read().await;
        let mut result = Vec::new();
        for (message_id, record) in messages.iter() {
            if record.session_id != session_id {
                continue;
            }
            let status = record
                .message
                .visibility
                .get(user_id)
                .copied()
                .unwrap_or(VisibilityStatus::Visible as i32);
            if status == visibility_status as i32 {
                result.push(message_id.clone());
            }
        }
        Ok(result)
    }
}
