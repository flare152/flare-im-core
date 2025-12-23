//! 命令处理器（编排层）- 轻量级，只负责编排领域服务

use std::sync::Arc;
use anyhow::Result;
use tracing::{debug, info};

use crate::application::commands::RegisterRouteCommand;
use crate::domain::service::RouteDomainService;
use crate::domain::model::{Svid, Endpoint};

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
        // 将字符串转换为值对象
        let svid = Svid::new(command.svid.clone())
            .map_err(|e| anyhow::anyhow!("Invalid SVID: {}", e))?;
        let endpoint = Endpoint::new(command.endpoint.clone())
            .map_err(|e| anyhow::anyhow!("Invalid endpoint: {}", e))?;
        
        let svid_str = svid.as_str().to_string();
        let endpoint_str = endpoint.as_str().to_string();
        debug!(svid = %svid_str, endpoint = %endpoint_str, "Handling register route command");

        self.domain_service
            .register_route(svid, endpoint)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to register route: {}", e))?;

        info!(svid = %svid_str, "Route registered successfully");
        Ok(())
    }
}

