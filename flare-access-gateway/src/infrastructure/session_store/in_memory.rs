use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use flare_server_core::error::Result;
use tokio::sync::RwLock;

use crate::domain::{Session, SessionStore};

#[derive(Default)]
pub struct InMemorySessionStore {
    inner: Arc<RwLock<HashMap<String, Session>>>,
}

impl InMemorySessionStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl SessionStore for InMemorySessionStore {
    async fn insert(&self, session: Session) -> Result<()> {
        let mut guard = self.inner.write().await;
        guard.insert(session.session_id.clone(), session);
        Ok(())
    }

    async fn remove(&self, session_id: &str) -> Result<Option<Session>> {
        let mut guard = self.inner.write().await;
        Ok(guard.remove(session_id))
    }

    async fn update_connection(
        &self,
        session_id: &str,
        connection_id: Option<String>,
    ) -> Result<()> {
        let mut guard = self.inner.write().await;
        if let Some(session) = guard.get_mut(session_id) {
            session.set_connection(connection_id);
        }
        Ok(())
    }

    async fn touch(&self, session_id: &str) -> Result<Option<Session>> {
        let mut guard = self.inner.write().await;
        if let Some(session) = guard.get_mut(session_id) {
            session.touch();
            return Ok(Some(session.clone()));
        }
        Ok(None)
    }

    async fn find_by_user(&self, user_id: &str) -> Result<Vec<Session>> {
        let guard = self.inner.read().await;
        Ok(guard
            .values()
            .filter(|session| session.user_id == user_id)
            .cloned()
            .collect())
    }

    async fn all(&self) -> Result<HashMap<String, Session>> {
        let guard = self.inner.read().await;
        Ok(guard.clone())
    }
}
