use std::sync::Arc;

use anyhow::Result;

use crate::domain::DeviceRouteRepository;
use crate::domain::entities::device_route::DeviceRoute;

/// 设备路由查询服务
pub struct DeviceRouteQueryService {
    repository: Arc<dyn DeviceRouteRepository>,
}

impl DeviceRouteQueryService {
    pub fn new(repository: Arc<dyn DeviceRouteRepository>) -> Self {
        Self { repository }
    }

    /// 获取某用户的所有设备路由
    pub async fn list_routes(&self, user_id: String) -> Result<Vec<DeviceRoute>> {
        self.repository.list_by_user(&user_id).await
    }

    /// 获取某用户的最佳设备路由（优先级优先，其次质量评分）
    pub async fn get_best_route(&self, user_id: String) -> Result<Option<DeviceRoute>> {
        let mut routes = self.repository.list_by_user(&user_id).await?;
        if routes.is_empty() {
            return Ok(None);
        }

        routes.sort_by(|a, b| {
            match b.device_priority.cmp(&a.device_priority) {
                std::cmp::Ordering::Equal => b.quality_score
                    .partial_cmp(&a.quality_score)
                    .unwrap_or(std::cmp::Ordering::Equal),
                other => other,
            }
        });

        Ok(routes.into_iter().next())
    }
}
