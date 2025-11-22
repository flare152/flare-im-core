use std::sync::Arc;

use anyhow::Result;
use tracing::{debug, info};

use crate::application::commands::RegisterRouteCommand;
use crate::application::queries::ResolveRouteQuery;
use crate::domain::service::RouteDomainService;

/// 路由命令处理器
pub struct RouteCommandHandler {
    domain_service: Arc<RouteDomainService>,
}

impl RouteCommandHandler {
    pub fn new(domain_service: Arc<RouteDomainService>) -> Self {
        Self { domain_service }
    }

    /// 处理注册路由命令
    pub async fn handle_register_route(&self, command: RegisterRouteCommand) -> Result<()> {
        let svid = command.svid.clone();
        let endpoint = command.endpoint.clone();
        debug!(svid = %svid, endpoint = %endpoint, "Handling register route command");
        
        self.domain_service
            .register_route(command.svid, command.endpoint)
            .await?;
        
        info!(svid = %svid, "Route registered successfully");
        Ok(())
    }
}

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
        let svid = query.svid.clone();
        debug!(svid = %svid, "Handling resolve route query");
        
        let endpoint = self.domain_service.resolve_route(query.svid).await?;
        
        if let Some(ref endpoint) = endpoint {
            debug!(svid = %svid, endpoint = %endpoint, "Route resolved");
        } else {
            debug!(svid = %svid, "Route not found");
        }
        
        Ok(endpoint)
    }
}

