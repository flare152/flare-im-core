use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::{TimeZone, Utc};
use redis::{AsyncCommands, aio::ConnectionManager};

use crate::config::SessionConfig;
use crate::domain::model::{
    Session, SessionBootstrapResult, SessionFilter, SessionParticipant, SessionSort,
    SessionSummary,
};
use crate::domain::repository::SessionRepository;
use async_trait::async_trait;

pub struct RedisSessionRepository {
    client: Arc<redis::Client>,
    config: Arc<SessionConfig>,
}

impl RedisSessionRepository {
    pub fn new(client: Arc<redis::Client>, config: Arc<SessionConfig>) -> Self {
        Self { client, config }
    }

    async fn connection(&self) -> Result<ConnectionManager> {
        Ok(ConnectionManager::new(self.client.as_ref().clone()).await?)
    }

    fn session_state_key(&self, session_id: &str) -> String {
        format!("{}:{}", self.config.session_state_prefix, session_id)
    }

    fn session_unread_key(&self, session_id: &str) -> String {
        format!("{}:{}", self.config.session_unread_prefix, session_id)
    }

    fn user_cursor_key(&self, user_id: &str) -> String {
        format!("{}:{}", self.config.user_cursor_prefix, user_id)
    }
}


#[async_trait]
impl SessionRepository for RedisSessionRepository {
    async fn load_bootstrap(
        &self,
        user_id: &str,
        client_cursor: &HashMap<String, i64>,
    ) -> Result<SessionBootstrapResult> {
        let mut conn = self.connection().await?;

        let cursor_key = self.user_cursor_key(user_id);
        let mut server_cursor: HashMap<String, i64> = conn
            .hgetall::<_, HashMap<String, String>>(&cursor_key)
            .await?
            .into_iter()
            .filter_map(|(k, v)| v.parse::<i64>().ok().map(|ts| (k, ts)))
            .collect();

        // merge client cursor hints to ensure we cover requested sessions
        for (session_id, ts) in client_cursor {
            server_cursor.entry(session_id.clone()).or_insert(*ts);
        }

        let mut summaries = Vec::new();

        for session_id in server_cursor.keys() {
            let state_key = self.session_state_key(session_id);
            let state: HashMap<String, String> = conn
                .hgetall::<_, HashMap<String, String>>(&state_key)
                .await
                .with_context(|| format!("load session state {}", session_id))?;

            if state.is_empty() {
                continue;
            }

            let unread_key = self.session_unread_key(session_id);
            let unread_raw: Option<String> = conn.hget(&unread_key, user_id.to_string()).await?;
            let unread: i32 = unread_raw
                .and_then(|v| v.parse::<i32>().ok())
                .unwrap_or_default();

            let last_ts = state
                .get("last_message_ts")
                .and_then(|v| v.parse::<i64>().ok());

            let summary = SessionSummary {
                session_id: session_id.clone(),
                session_type: state.get("session_type").cloned(),
                business_type: state.get("business_type").cloned(),
                last_message_id: state.get("last_message_id").cloned(),
                last_message_time: last_ts.and_then(|ts| Utc.timestamp_millis_opt(ts).single()),
                last_sender_id: state.get("last_sender_id").cloned(),
                last_message_type: state
                    .get("last_message_type")
                    .and_then(|v| v.parse::<i32>().ok()),
                last_content_type: state.get("last_content_type").cloned(),
                unread_count: unread,
                metadata: HashMap::new(),
                server_cursor_ts: last_ts.or_else(|| server_cursor.get(session_id).copied()),
                display_name: state.get("display_name").cloned(),
            };

            summaries.push(summary);
        }

        summaries.sort_by(|a, b| {
            let at = a.server_cursor_ts.unwrap_or_default();
            let bt = b.server_cursor_ts.unwrap_or_default();
            bt.cmp(&at)
        });

        Ok(SessionBootstrapResult {
            summaries,
            recent_messages: Vec::new(),
            cursor_map: server_cursor,
            policy: self.config.default_policy.clone(),
        })
    }

    async fn update_cursor(&self, user_id: &str, session_id: &str, ts: i64) -> Result<()> {
        let mut conn = self.connection().await?;
        let cursor_key = self.user_cursor_key(user_id);
        let _: () = conn.hset(cursor_key, session_id, ts).await?;
        Ok(())
    }

    // Redis repository不支持会话管理操作，这些操作需要在PostgreSQL repository中实现
    async fn create_session(&self, _session: &Session) -> Result<()> {
        Err(anyhow::anyhow!(
            "RedisSessionRepository does not support create_session. Use PostgresSessionRepository instead."
        ))
    }

    async fn get_session(&self, _session_id: &str) -> Result<Option<Session>> {
        Err(anyhow::anyhow!(
            "RedisSessionRepository does not support get_session. Use PostgresSessionRepository instead."
        ))
    }

    async fn update_session(&self, _session: &Session) -> Result<()> {
        Err(anyhow::anyhow!(
            "RedisSessionRepository does not support update_session. Use PostgresSessionRepository instead."
        ))
    }

    async fn delete_session(&self, _session_id: &str, _hard_delete: bool) -> Result<()> {
        Err(anyhow::anyhow!(
            "RedisSessionRepository does not support delete_session. Use PostgresSessionRepository instead."
        ))
    }

    async fn manage_participants(
        &self,
        _session_id: &str,
        _to_add: &[SessionParticipant],
        _to_remove: &[String],
        _role_updates: &[(String, Vec<String>)],
    ) -> Result<Vec<SessionParticipant>> {
        Err(anyhow::anyhow!(
            "RedisSessionRepository does not support manage_participants. Use PostgresSessionRepository instead."
        ))
    }

    async fn batch_acknowledge(
        &self,
        user_id: &str,
        cursors: &[(String, i64)],
    ) -> Result<()> {
        // Redis repository支持批量确认，因为这只是更新光标
        let mut conn = self.connection().await?;
        let cursor_key = self.user_cursor_key(user_id);
        for (session_id, ts) in cursors {
            let _: () = conn.hset(&cursor_key, session_id, ts).await?;
        }
        Ok(())
    }

    async fn search_sessions(
        &self,
        _user_id: Option<&str>,
        _filters: &[SessionFilter],
        _sort: &[SessionSort],
        _limit: usize,
        _offset: usize,
    ) -> Result<(Vec<SessionSummary>, usize)> {
        Err(anyhow::anyhow!(
            "RedisSessionRepository does not support search_sessions. Use PostgresSessionRepository instead."
        ))
    }

    async fn mark_as_read(
        &self,
        _user_id: &str,
        _session_id: &str,
        _seq: i64,
    ) -> Result<()> {
        Err(anyhow::anyhow!(
            "RedisSessionRepository does not support mark_as_read. Use PostgresSessionRepository instead."
        ))
    }

    async fn get_unread_count(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> Result<i32> {
        // Redis repository 支持读取未读数（从缓存）
        let mut conn = self.connection().await?;
        let unread_key = self.session_unread_key(session_id);
        let unread_raw: Option<String> = conn.hget(&unread_key, user_id.to_string()).await?;
        let unread: i32 = unread_raw
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or_default();
        Ok(unread)
    }
}
