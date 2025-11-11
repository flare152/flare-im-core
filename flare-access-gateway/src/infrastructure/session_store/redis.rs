use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Error};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use flare_server_core::error::{ErrorCode, InfraResult, InfraResultExt, Result};
use redis::{AsyncCommands, aio::ConnectionManager};
use serde_json::{Value, json};
use tracing::info;

use crate::domain::SessionStore;
use crate::domain::session::Session;

const SESSION_PREFIX: &str = "session:";
const USER_SESSIONS_PREFIX: &str = "user_sessions:";
const SESSION_INDEX_KEY: &str = "session:index";

pub struct RedisSessionStore {
    client: Arc<redis::Client>,
    ttl_seconds: u64,
}

impl RedisSessionStore {
    pub fn new(client: Arc<redis::Client>, ttl_seconds: u64) -> Self {
        Self {
            client,
            ttl_seconds,
        }
    }

    async fn connection(&self) -> InfraResult<ConnectionManager> {
        ConnectionManager::new(self.client.as_ref().clone())
            .await
            .map_err(Error::new)
    }

    fn ttl_seconds_i64(&self) -> InfraResult<i64> {
        i64::try_from(self.ttl_seconds).map_err(Error::new)
    }

    fn session_key(session_id: &str) -> String {
        format!("{}{}", SESSION_PREFIX, session_id)
    }

    fn user_set_key(user_id: &str) -> String {
        format!("{}{}", USER_SESSIONS_PREFIX, user_id)
    }

    fn serialize(session: &Session) -> Value {
        json!({
            "session_id": session.session_id,
            "user_id": session.user_id,
            "device_id": session.device_id,
            "route_server": session.route_server,
            "gateway_id": session.gateway_id,
            "connection_id": session.connection_id,
            "last_heartbeat": session.last_heartbeat.timestamp_millis(),
        })
    }

    fn deserialize(value: Value) -> InfraResult<Session> {
        let session_id = value
            .get("session_id")
            .and_then(Value::as_str)
            .context("missing session_id")?
            .to_string();
        let user_id = value
            .get("user_id")
            .and_then(Value::as_str)
            .context("missing user_id")?
            .to_string();
        let device_id = value
            .get("device_id")
            .and_then(Value::as_str)
            .context("missing device_id")?
            .to_string();
        let route_server = value
            .get("route_server")
            .and_then(Value::as_str)
            .map(|s| s.to_string());
        let gateway_id = value
            .get("gateway_id")
            .and_then(Value::as_str)
            .context("missing gateway_id")?
            .to_string();
        let connection_id = value
            .get("connection_id")
            .and_then(Value::as_str)
            .map(|s| s.to_string());
        let heartbeat_ms = value
            .get("last_heartbeat")
            .and_then(Value::as_i64)
            .context("missing last_heartbeat")?;
        let last_heartbeat = DateTime::<Utc>::from_timestamp_millis(heartbeat_ms)
            .context("invalid heartbeat timestamp")?;

        Ok(Session {
            session_id,
            user_id,
            device_id,
            route_server,
            gateway_id,
            connection_id,
            last_heartbeat,
        })
    }

    async fn fetch_session(
        &self,
        conn: &mut ConnectionManager,
        session_id: &str,
    ) -> InfraResult<Option<Session>> {
        let key = Self::session_key(session_id);
        let payload: Option<String> = conn.get(&key).await.context("failed to get session")?;
        if let Some(raw) = payload {
            let value: Value = serde_json::from_str(&raw).context("invalid session json")?;
            Self::deserialize(value).map(Some)
        } else {
            Ok(None)
        }
    }

    async fn persist_session(
        &self,
        conn: &mut ConnectionManager,
        session: &Session,
    ) -> InfraResult<()> {
        let key = Self::session_key(&session.session_id);
        let payload = Self::serialize(session);
        let json = serde_json::to_string(&payload).context("failed to encode session json")?;
        let _: () = conn
            .set_ex(&key, json, self.ttl_seconds)
            .await
            .context("failed to set session")?;
        Ok(())
    }

    async fn index_session(
        &self,
        conn: &mut ConnectionManager,
        session: &Session,
    ) -> InfraResult<()> {
        let _: usize = conn
            .sadd(SESSION_INDEX_KEY, &session.session_id)
            .await
            .context("failed to add session to index")?;
        let _: usize = conn
            .sadd(Self::user_set_key(&session.user_id), &session.session_id)
            .await
            .context("failed to add session to user set")?;
        let ttl = self.ttl_seconds_i64()?;
        let _: bool = conn
            .expire(Self::user_set_key(&session.user_id), ttl)
            .await
            .context("failed to set user set ttl")?;
        let _: bool = conn
            .expire(SESSION_INDEX_KEY, ttl)
            .await
            .context("failed to set session index ttl")?;
        Ok(())
    }

    async fn deindex_session(
        &self,
        conn: &mut ConnectionManager,
        session: &Session,
    ) -> InfraResult<()> {
        let _: usize = conn
            .srem(SESSION_INDEX_KEY, &session.session_id)
            .await
            .context("failed to remove session from index")?;
        let _: usize = conn
            .srem(Self::user_set_key(&session.user_id), &session.session_id)
            .await
            .context("failed to remove session from user set")?;
        Ok(())
    }
}

#[async_trait]
impl SessionStore for RedisSessionStore {
    async fn insert(&self, session: Session) -> Result<()> {
        let mut conn = self.connection().await.into_flare(
            ErrorCode::ServiceUnavailable,
            "failed to open redis connection",
        )?;

        self.persist_session(&mut conn, &session)
            .await
            .into_flare(ErrorCode::DatabaseError, "failed to persist session")?;
        self.index_session(&mut conn, &session)
            .await
            .into_flare(ErrorCode::DatabaseError, "failed to index session")?;

        info!(session_id = %session.session_id, user_id = %session.user_id, "session stored in redis");
        Ok(())
    }

    async fn remove(&self, session_id: &str) -> Result<Option<Session>> {
        let mut conn = self.connection().await.into_flare(
            ErrorCode::ServiceUnavailable,
            "failed to open redis connection",
        )?;

        match self
            .fetch_session(&mut conn, session_id)
            .await
            .into_flare(ErrorCode::DatabaseError, "failed to fetch session")?
        {
            Some(session) => {
                let _: usize = conn
                    .del(Self::session_key(session_id))
                    .await
                    .context("failed to delete session")
                    .into_flare(ErrorCode::DatabaseError, "failed to delete session")?;
                self.deindex_session(&mut conn, &session)
                    .await
                    .into_flare(ErrorCode::DatabaseError, "failed to deindex session")?;
                Ok(Some(session))
            }
            None => Ok(None),
        }
    }

    async fn update_connection(
        &self,
        session_id: &str,
        connection_id: Option<String>,
    ) -> Result<()> {
        let mut conn = self.connection().await.into_flare(
            ErrorCode::ServiceUnavailable,
            "failed to open redis connection",
        )?;

        if let Some(mut session) = self
            .fetch_session(&mut conn, session_id)
            .await
            .into_flare(ErrorCode::DatabaseError, "failed to fetch session")?
        {
            session.connection_id = connection_id;
            self.persist_session(&mut conn, &session)
                .await
                .into_flare(ErrorCode::DatabaseError, "failed to persist session")?;
        }
        Ok(())
    }

    async fn touch(&self, session_id: &str) -> Result<Option<Session>> {
        let mut conn = self.connection().await.into_flare(
            ErrorCode::ServiceUnavailable,
            "failed to open redis connection",
        )?;

        match self
            .fetch_session(&mut conn, session_id)
            .await
            .into_flare(ErrorCode::DatabaseError, "failed to fetch session")?
        {
            Some(mut session) => {
                session.last_heartbeat = Utc::now();
                self.persist_session(&mut conn, &session)
                    .await
                    .into_flare(ErrorCode::DatabaseError, "failed to persist session")?;
                Ok(Some(session))
            }
            None => Ok(None),
        }
    }

    async fn find_by_user(&self, user_id: &str) -> Result<Vec<Session>> {
        let mut conn = self.connection().await.into_flare(
            ErrorCode::ServiceUnavailable,
            "failed to open redis connection",
        )?;

        let key = Self::user_set_key(user_id);
        let session_ids: Vec<String> = conn
            .smembers(&key)
            .await
            .context("failed to fetch user session ids")
            .into_flare(ErrorCode::DatabaseError, "failed to fetch user session ids")?;

        let mut sessions = Vec::with_capacity(session_ids.len());
        for session_id in session_ids {
            match self
                .fetch_session(&mut conn, &session_id)
                .await
                .into_flare(ErrorCode::DatabaseError, "failed to fetch session")?
            {
                Some(session) => sessions.push(session),
                None => {
                    let _: usize = conn
                        .srem(&key, &session_id)
                        .await
                        .context("failed to cleanup stale session reference")
                        .into_flare(
                            ErrorCode::DatabaseError,
                            "failed to cleanup stale session reference",
                        )?;
                }
            }
        }
        Ok(sessions)
    }

    async fn all(&self) -> Result<HashMap<String, Session>> {
        let mut conn = self.connection().await.into_flare(
            ErrorCode::ServiceUnavailable,
            "failed to open redis connection",
        )?;

        let session_ids: Vec<String> = conn
            .smembers(SESSION_INDEX_KEY)
            .await
            .context("failed to fetch session index")
            .into_flare(ErrorCode::DatabaseError, "failed to fetch session index")?;
        let mut map: HashMap<String, Session> = HashMap::with_capacity(session_ids.len());
        for session_id in session_ids {
            match self
                .fetch_session(&mut conn, &session_id)
                .await
                .into_flare(ErrorCode::DatabaseError, "failed to fetch session")?
            {
                Some(session) => {
                    map.insert(session_id, session);
                }
                None => {
                    let _: usize = conn
                        .srem(SESSION_INDEX_KEY, &session_id)
                        .await
                        .context("failed to cleanup stale session index entry")
                        .into_flare(
                            ErrorCode::DatabaseError,
                            "failed to cleanup stale session index entry",
                        )?;
                }
            }
        }
        Ok(map)
    }
}
