//! 内存版读侧存储实现
//!
//! 为了简化依赖并修复编译问题，这里提供一个纯内存的存储实现。
//! 该实现仅用于开发和 CI 环境，生产部署需替换为真实的数据存储。

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use flare_proto::storage::{Message, SessionInfo, VisibilityStatus};
use tokio::sync::RwLock;
use tracing::warn;

use crate::domain::{
    MessageStorage, MessageUpdate, SessionStorage, SessionUpdate, VisibilityStorage,
};

#[derive(Default, Clone)]
struct StoredMessage {
    session_id: String,
    message: Message,
    updated_at: i64,
}

#[derive(Default, Clone)]
struct StoredSession {
    session: SessionInfo,
    updated_at: i64,
}

#[derive(Default)]
pub struct MongoMessageStorage {
    messages: Arc<RwLock<HashMap<String, StoredMessage>>>,
    session_index: Arc<RwLock<HashMap<String, Vec<String>>>>,
    sessions: Arc<RwLock<HashMap<String, StoredSession>>>,
    visibility: Arc<RwLock<HashMap<(String, String), VisibilityStatus>>>,
}

impl MongoMessageStorage {
    pub async fn new(_uri: &str, _database: &str) -> Result<Self, Box<dyn std::error::Error>> {
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
    ) -> Result<(), Box<dyn std::error::Error>> {
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
    ) -> Result<Vec<Message>, Box<dyn std::error::Error>> {
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
    ) -> Result<Option<Message>, Box<dyn std::error::Error>> {
        let messages = self.messages.read().await;
        Ok(messages
            .get(message_id)
            .map(|record| record.message.clone()))
    }

    async fn update_message(
        &self,
        message_id: &str,
        updates: MessageUpdate,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut messages = self.messages.write().await;
        if let Some(record) = messages.get_mut(message_id) {
            let message = &mut record.message;

            if let Some(is_recalled) = updates.is_recalled {
                message.is_recalled = is_recalled;
            }
            if let Some(recalled_at) = updates.recalled_at {
                message.recalled_at = Some(recalled_at);
            }
            if let Some(reason) = updates.recall_reason {
                message.recall_reason = reason;
            }
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
            record.updated_at = Utc::now().timestamp_millis();
        }

        Ok(())
    }

    async fn batch_update_visibility(
        &self,
        message_ids: &[String],
        user_id: &str,
        visibility: VisibilityStatus,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let mut updated = 0usize;
        let mut vis_map = self.visibility.write().await;
        for message_id in message_ids {
            vis_map.insert((message_id.clone(), user_id.to_string()), visibility);
            updated += 1;
        }
        Ok(updated)
    }
}

#[async_trait::async_trait]
impl SessionStorage for MongoMessageStorage {
    async fn create_session(
        &self,
        session: &SessionInfo,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut sessions = self.sessions.write().await;
        sessions.insert(
            session.session_id.clone(),
            StoredSession {
                session: session.clone(),
                updated_at: Utc::now().timestamp_millis(),
            },
        );
        Ok(())
    }

    async fn get_session(
        &self,
        session_id: &str,
    ) -> Result<Option<SessionInfo>, Box<dyn std::error::Error>> {
        let sessions = self.sessions.read().await;
        Ok(sessions
            .get(session_id)
            .map(|record| record.session.clone()))
    }

    async fn update_session(
        &self,
        session_id: &str,
        updates: SessionUpdate,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut sessions = self.sessions.write().await;
        let Some(record) = sessions.get_mut(session_id) else {
            warn!(session_id, "update_session on missing session");
            return Ok(());
        };

        if let Some(last_message_id) = updates.last_message_id {
            record.session.last_message_id = last_message_id;
        }
        if let Some(last_message_time) = updates.last_message_time {
            record.session.last_message_time = Some(last_message_time);
        }
        if let Some(unread) = updates.unread_count {
            for (user, count) in unread {
                record.session.unread_count.insert(user, count);
            }
        }
        if let Some(status) = updates.status {
            record.session.status = status;
        }
        if let Some(metadata) = updates.metadata {
            for (k, v) in metadata {
                record.session.metadata.insert(k, v);
            }
        }

        record.updated_at = Utc::now().timestamp_millis();
        Ok(())
    }

    async fn query_user_sessions(
        &self,
        user_id: &str,
        business_type: Option<&str>,
        limit: i32,
        _cursor: Option<&str>,
    ) -> Result<(Vec<SessionInfo>, Option<String>), Box<dyn std::error::Error>> {
        let sessions = self.sessions.read().await;
        let mut collected: Vec<&StoredSession> = sessions
            .values()
            .filter(|record| {
                let participates = record.session.participants.iter().any(|p| p == user_id);
                let matches_business = business_type
                    .map(|b| record.session.business_type == b)
                    .unwrap_or(true);
                participates && matches_business
            })
            .collect();

        collected.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        collected.truncate(limit.max(0) as usize);

        let sessions = collected
            .into_iter()
            .map(|record| record.session.clone())
            .collect();
        Ok((sessions, None))
    }
}

#[async_trait::async_trait]
impl VisibilityStorage for MongoMessageStorage {
    async fn set_visibility(
        &self,
        message_id: &str,
        user_id: &str,
        _session_id: &str,
        visibility: VisibilityStatus,
    ) -> Result<(), Box<dyn std::error::Error>> {
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
    ) -> Result<Option<VisibilityStatus>, Box<dyn std::error::Error>> {
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
    ) -> Result<usize, Box<dyn std::error::Error>> {
        self.batch_update_visibility(message_ids, user_id, visibility)
            .await
    }

    async fn query_visible_message_ids(
        &self,
        user_id: &str,
        session_id: &str,
        visibility_status: VisibilityStatus,
    ) -> Result<Vec<String>, Box<dyn std::error::Error>> {
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
