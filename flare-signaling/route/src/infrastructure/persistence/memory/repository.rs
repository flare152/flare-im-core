use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use flare_server_core::error::Result;
use tokio::sync::RwLock;

use crate::domain::repositories::RouteRepository;

pub struct InMemoryRouteRepository {
    routes: Arc<RwLock<HashMap<String, String>>>,
}

impl InMemoryRouteRepository {
    pub fn new(initial: Vec<(String, String)>) -> Self {
        Self {
            routes: Arc::new(RwLock::new(initial.into_iter().collect())),
        }
    }
}

#[async_trait]
impl RouteRepository for InMemoryRouteRepository {
    async fn upsert(&self, svid: &str, endpoint: &str) -> Result<()> {
        let mut map = self.routes.write().await;
        map.insert(svid.to_string(), endpoint.to_string());
        Ok(())
    }

    async fn resolve(&self, svid: &str) -> Result<Option<String>> {
        let map = self.routes.read().await;
        Ok(map.get(svid).cloned())
    }
}
