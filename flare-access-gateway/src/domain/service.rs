//! 领域服务
//!
//! 包含领域逻辑和业务流程

use std::sync::Arc;

use crate::domain::repositories::{ConnectionQuery, SessionStore, SignalingGateway};

/// 网关领域服务
pub struct GatewayService {
    session_store: Arc<dyn SessionStore>,
    signaling_gateway: Arc<dyn SignalingGateway>,
    connection_query: Arc<dyn ConnectionQuery>,
}

impl GatewayService {
    pub fn new(
        session_store: Arc<dyn SessionStore>,
        signaling_gateway: Arc<dyn SignalingGateway>,
        connection_query: Arc<dyn ConnectionQuery>,
    ) -> Self {
        Self {
            session_store,
            signaling_gateway,
            connection_query,
        }
    }

    /// 获取会话存储
    pub fn session_store(&self) -> Arc<dyn SessionStore> {
        Arc::clone(&self.session_store)
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

