//! 应用服务层

use std::sync::Arc;

use crate::domain::service::PushService;

/// 推送应用服务
pub struct PushApplication {
    service: Arc<PushService>,
}

impl PushApplication {
    pub fn new(service: Arc<PushService>) -> Self {
        Self { service }
    }

    /// 获取领域服务
    pub fn service(&self) -> Arc<PushService> {
        Arc::clone(&self.service)
    }
}

