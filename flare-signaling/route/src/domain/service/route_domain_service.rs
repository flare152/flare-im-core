//! 路由领域服务
//!
//! 负责路由的核心业务逻辑：分片选择、负载均衡、流控、跨机房选择

use std::sync::Arc;
use anyhow::Result;
use tracing::{debug, warn};

use crate::domain::model::{Route, Svid, Endpoint, RouteError};
use crate::domain::repository::RouteRepository;

/// 路由领域服务
///
/// 职责：
/// - 注册路由（维护业务不变式）
/// - 解析路由（业务逻辑）
/// - 删除路由
pub struct RouteDomainService {
    repository: Arc<dyn RouteRepository>,
}

impl RouteDomainService {
    pub fn new(repository: Arc<dyn RouteRepository>) -> Self {
        Self { repository }
    }

    /// 注册路由（Command 侧）
    ///
    /// # 业务规则
    /// - SVID 必须唯一
    /// - 如果已存在，更新端点并递增版本
    pub async fn register_route(
        &self,
        svid: Svid,
        endpoint: Endpoint,
    ) -> Result<(), RouteError> {
        debug!(svid = %svid, endpoint = %endpoint, "Registering route");

        // 检查是否已存在
        if let Ok(Some(mut existing)) = self.repository.find_by_svid(svid.as_str()).await {
            warn!(
                svid = %svid,
                old_endpoint = %existing.endpoint(),
                new_endpoint = %endpoint,
                "Route already exists, updating"
            );
            // 更新端点
            existing.update_endpoint(endpoint)?;
            self.repository.save(existing).await
                .map_err(|e| RouteError::InvalidEndpoint(format!("Failed to save route: {}", e)))?;
        } else {
            // 创建新路由
            let route = Route::new(svid, endpoint);
            self.repository.save(route).await
                .map_err(|e| RouteError::InvalidEndpoint(format!("Failed to save route: {}", e)))?;
        }

        Ok(())
    }

    /// 解析路由（Query 侧）
    ///
    /// 返回端点地址，如果不存在则返回 None
    pub async fn resolve_route(&self, svid: &Svid) -> Result<Option<Endpoint>, RouteError> {
        debug!(svid = %svid, "Resolving route");

        match self.repository.find_by_svid(svid.as_str()).await {
            Ok(Some(route)) => {
                debug!(svid = %svid, endpoint = %route.endpoint(), "Route found");
                Ok(Some(route.endpoint().clone()))
            }
            Ok(None) => {
                debug!(svid = %svid, "Route not found");
                Ok(None)
            }
            Err(e) => Err(RouteError::InvalidEndpoint(format!("Repository error: {}", e))),
        }
    }

    /// 删除路由
    pub async fn unregister_route(&self, svid: &Svid) -> Result<(), RouteError> {
        debug!(svid = %svid, "Unregistering route");

        self.repository.delete(svid.as_str()).await
            .map_err(|e| RouteError::InvalidEndpoint(format!("Failed to delete route: {}", e)))?;

        Ok(())
    }
}

