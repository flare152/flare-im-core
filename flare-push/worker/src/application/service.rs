//! 应用服务层

use std::sync::Arc;

use crate::domain::service::PushDomainService;

/// 应用服务（保留用于未来扩展）
pub struct PushApplication {
    domain_service: Arc<PushDomainService>,
}

impl PushApplication {
    pub fn new(domain_service: Arc<PushDomainService>) -> Self {
        Self { domain_service }
    }

    /// 获取领域服务
    pub fn domain_service(&self) -> Arc<PushDomainService> {
        Arc::clone(&self.domain_service)
    }
}

