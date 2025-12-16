use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::domain::DeviceRouteRepository;
use crate::domain::entities::device_route::DeviceRoute;

/// 内存版设备路由仓储，实现用于开发和单测阶段
pub struct InMemoryDeviceRouteRepository {
    // key: user_id, value: Vec<DeviceRoute>
    routes: Arc<RwLock<HashMap<String, Vec<DeviceRoute>>>>,
}

impl InMemoryDeviceRouteRepository {
    pub fn new() -> Self {
        Self {
            routes: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl DeviceRouteRepository for InMemoryDeviceRouteRepository {
    async fn upsert(&self, route: DeviceRoute) -> Result<()> {
        let mut map = self.routes.write().await;
        let user_routes = map.entry(route.user_id.clone()).or_insert_with(Vec::new);

        // 如果已有同一 device_id 的路由，则替换
        if let Some(existing) = user_routes
            .iter_mut()
            .find(|r| r.device_id == route.device_id)
        {
            *existing = route;
        } else {
            user_routes.push(route);
        }

        Ok(())
    }

    async fn remove(&self, user_id: &str, device_id: &str) -> Result<()> {
        let mut map = self.routes.write().await;
        if let Some(user_routes) = map.get_mut(user_id) {
            user_routes.retain(|r| r.device_id != device_id);
        }
        Ok(())
    }

    async fn remove_all(&self, user_id: &str) -> Result<()> {
        let mut map = self.routes.write().await;
        map.remove(user_id);
        Ok(())
    }

    async fn list_by_user(&self, user_id: &str) -> Result<Vec<DeviceRoute>> {
        let map = self.routes.read().await;
        Ok(map.get(user_id).cloned().unwrap_or_default())
    }
}
