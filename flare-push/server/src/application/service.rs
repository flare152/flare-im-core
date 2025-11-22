//! 应用服务层

use std::sync::Arc;

use crate::domain::service::PushDomainService;

/// 推送应用服务
pub struct PushApplication {
    service: Arc<PushDomainService>,
}

impl PushApplication {
    pub fn new(service: Arc<PushDomainService>) -> Self {
        Self { service }
    }

    /// 获取领域服务
    pub fn service(&self) -> Arc<PushDomainService> {
        Arc::clone(&self.service)
    }
}

