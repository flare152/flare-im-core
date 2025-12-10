//! 领域服务
//!
//! 包含领域逻辑和业务流程

pub mod push_domain_service;
pub mod session_domain_service;
pub mod connection_quality_service;
pub mod multi_device_push_service;

#[cfg(test)]
mod push_domain_service_test;

use std::sync::Arc;

use crate::domain::repository::{ConnectionQuery, SignalingGateway};

pub use push_domain_service::{DomainPushResult, PushDomainService};
pub use session_domain_service::SessionDomainService;
pub use connection_quality_service::{ConnectionQualityService, ConnectionQualityMetrics, QualityLevel};
pub use multi_device_push_service::{MultiDevicePushService, PushStrategy, PushTarget};

/// 网关领域服务
pub struct GatewayService {
    signaling_gateway: Arc<dyn SignalingGateway>,
    connection_query: Arc<dyn ConnectionQuery>,
}

impl GatewayService {
    pub fn new(
        signaling_gateway: Arc<dyn SignalingGateway>,
        connection_query: Arc<dyn ConnectionQuery>,
    ) -> Self {
        Self {
            signaling_gateway,
            connection_query,
        }
    }

    /// 获取信令网关
    pub fn signaling_gateway(&self) -> Arc<dyn SignalingGateway> {
        Arc::clone(&self.signaling_gateway)
    }

    /// 获取连接查询
    pub fn connection_query(&self) -> Arc<dyn ConnectionQuery> {
        Arc::clone(&self.connection_query)
    }
}

