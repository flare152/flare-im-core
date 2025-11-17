//! 应用服务层
//!
//! 提供应用层服务，协调领域服务和命令/查询服务

use std::sync::Arc;

use crate::domain::service::GatewayService;

/// 应用服务
pub struct GatewayApplication {
    service: Arc<GatewayService>,
}

impl GatewayApplication {
    pub fn new(service: Arc<GatewayService>) -> Self {
        Self { service }
    }

    /// 获取领域服务
    pub fn service(&self) -> Arc<GatewayService> {
        Arc::clone(&self.service)
    }
}

