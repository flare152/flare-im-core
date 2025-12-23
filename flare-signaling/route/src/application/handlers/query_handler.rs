//! 查询处理器（查询侧）- 直接调用基础设施层或领域服务

use std::sync::Arc;
use anyhow::Result;
use tracing::debug;

use crate::application::queries::ResolveRouteQuery;
use crate::domain::service::RouteDomainService;
use crate::domain::model::Svid;

/// 路由查询处理器
pub struct RouteQueryHandler {
    domain_service: Arc<RouteDomainService>,
}

impl RouteQueryHandler {
    pub fn new(domain_service: Arc<RouteDomainService>) -> Self {
        Self { domain_service }
    }

    /// 处理解析路由查询
    pub async fn handle_resolve_route(&self, query: ResolveRouteQuery) -> Result<Option<String>> {
        // 将字符串转换为值对象
        let svid = Svid::new(query.svid.clone())
            .map_err(|e| anyhow::anyhow!("Invalid SVID: {}", e))?;
        
        debug!(svid = %svid, "Handling resolve route query");

        let endpoint = self.domain_service.resolve_route(&svid).await
            .map_err(|e| anyhow::anyhow!("Failed to resolve route: {}", e))?;

        if let Some(ref endpoint) = endpoint {
            debug!(svid = %svid, endpoint = %endpoint, "Route resolved");
            Ok(Some(endpoint.as_str().to_string()))
        } else {
            debug!(svid = %svid, "Route not found");
            Ok(None)
        }
    }
}

