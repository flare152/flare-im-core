use std::sync::Arc;

use anyhow::Result;
use tracing::{debug, warn};

use crate::domain::model::Route;
use crate::domain::repository::RouteRepository;

/// 路由领域服务
pub struct RouteDomainService {
    repository: Arc<dyn RouteRepository>,
}

impl RouteDomainService {
    pub fn new(repository: Arc<dyn RouteRepository>) -> Self {
        Self { repository }
    }

    /// 注册路由
    pub async fn register_route(&self, svid: String, endpoint: String) -> Result<()> {
        debug!(svid = %svid, endpoint = %endpoint, "Registering route");
        
        let route = Route::new(svid.clone(), endpoint.clone());
        
        // 检查是否已存在
        if let Some(existing) = self.repository.find_by_svid(&svid).await? {
            warn!(
                svid = %svid,
                old_endpoint = %existing.endpoint,
                new_endpoint = %endpoint,
                "Route already exists, updating"
            );
        }
        
        self.repository.save(route).await?;
        
        Ok(())
    }

    /// 解析路由
    pub async fn resolve_route(&self, svid: String) -> Result<Option<String>> {
        debug!(svid = %svid, "Resolving route");
        
        match self.repository.find_by_svid(&svid).await? {
            Some(route) => {
                debug!(svid = %svid, endpoint = %route.endpoint, "Route found");
                Ok(Some(route.endpoint))
            }
            None => {
                debug!(svid = %svid, "Route not found");
                Ok(None)
            }
        }
    }

    /// 删除路由
    pub async fn unregister_route(&self, svid: String) -> Result<()> {
        debug!(svid = %svid, "Unregistering route");
        
        self.repository.delete(&svid).await?;
        
        Ok(())
    }
}

