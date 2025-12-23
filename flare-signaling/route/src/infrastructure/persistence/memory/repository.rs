use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::domain::model::Route;
use crate::domain::repository::RouteRepository;

pub struct InMemoryRouteRepository {
    routes: Arc<RwLock<HashMap<String, Route>>>,
}

impl InMemoryRouteRepository {
    pub fn new() -> Self {
        Self {
            routes: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl RouteRepository for InMemoryRouteRepository {
    async fn save(&self, route: Route) -> Result<()> {
        let mut map = self.routes.write().await;
        // 使用 SVID 作为 key
        let svid_key = route.svid().as_str().to_string();
        map.insert(svid_key, route);
        Ok(())
    }

    async fn find_by_svid(&self, svid: &str) -> Result<Option<Route>> {
        let map = self.routes.read().await;
        Ok(map.get(svid).cloned())
    }

    async fn delete(&self, svid: &str) -> Result<()> {
        let mut map = self.routes.write().await;
        map.remove(svid);
        Ok(())
    }
}
