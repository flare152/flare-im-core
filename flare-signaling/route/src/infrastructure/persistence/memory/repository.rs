use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::RwLock;
use async_trait::async_trait;

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
        map.insert(route.svid.clone(), route);
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
