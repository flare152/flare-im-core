//! 连接管理应用服务
//!
//! 处理连接生命周期的业务流程编排

use std::sync::Arc;
use flare_server_core::error::Result;  // 使用 flare_server_core 的 Result
use tracing::{info, warn, instrument};

use crate::domain::service::SessionDomainService;
use crate::domain::repository::ConnectionQuery;

/// 连接管理应用服务
///
/// 职责：
/// - 编排连接建立流程
/// - 编排连接断开流程
/// - 协调会话管理和连接管理
pub struct ConnectionApplicationService {
    session_domain_service: Arc<SessionDomainService>,
    connection_query: Arc<dyn ConnectionQuery>,
    metrics: Arc<flare_im_core::metrics::AccessGatewayMetrics>,
}

impl ConnectionApplicationService {
    pub fn new(
        session_domain_service: Arc<SessionDomainService>,
        connection_query: Arc<dyn ConnectionQuery>,
        metrics: Arc<flare_im_core::metrics::AccessGatewayMetrics>,
    ) -> Self {
        Self {
            session_domain_service,
            connection_query,
            metrics,
        }
    }

    /// 处理连接建立
    ///
    /// 流程：
    /// 1. 记录指标
    /// 2. 注册会话到 Signaling Online
    /// 3. 记录日志
    #[instrument(skip(self), fields(connection_id, user_id, device_id))]
    pub async fn handle_connect(
        &self,
        connection_id: &str,
        user_id: &str,
        device_id: &str,
        active_connections: usize,
    ) -> Result<String> {
        // 更新活跃连接数
        self.metrics.connections_active.set(active_connections as i64);

        info!(
            user_id = %user_id,
            device_id = %device_id,
            connection_id = %connection_id,
            active_connections = active_connections,
            "Connection established"
        );

        // 注册会话到 Signaling Online
        match self.session_domain_service
            .register_session(user_id, device_id, Some(connection_id))
            .await
        {
            Ok(session_id) => {
                info!(
                    user_id = %user_id,
                    connection_id = %connection_id,
                    session_id = %session_id,
                    "Online status registered"
                );
                Ok(session_id)
            }
            Err(err) => {
                warn!(
                    ?err,
                    user_id = %user_id,
                    connection_id = %connection_id,
                    "Failed to register online status"
                );
                Err(err)
            }
        }
    }

    /// 处理连接断开
    ///
    /// 流程：
    /// 1. 记录指标
    /// 2. 检查用户是否还有其他连接
    /// 3. 如果没有其他连接，注销会话
    #[instrument(skip(self), fields(connection_id, user_id))]
    pub async fn handle_disconnect(
        &self,
        connection_id: &str,
        user_id: &str,
        active_connections: usize,
        has_other_connections: bool,
    ) -> Result<()> {
        // 记录连接断开指标
        self.metrics.connection_disconnected_total.inc();

        // 更新活跃连接数
        self.metrics.connections_active.set(active_connections as i64);

        info!(
            connection_id = %connection_id,
            active_connections = active_connections,
            "Connection disconnected"
        );

        // 如果没有其他本地连接，注销会话
        if !has_other_connections {
            if let Err(err) = self.session_domain_service
                .unregister_session(user_id, None)
                .await
            {
                warn!(
                    ?err,
                    user_id = %user_id,
                    connection_id = %connection_id,
                    "Failed to unregister online status"
                );
                return Err(err);
            }
        }

        info!(
            user_id = %user_id,
            connection_id = %connection_id,
            "User disconnected"
        );

        Ok(())
    }

    /// 刷新会话心跳
    #[instrument(skip(self), fields(connection_id, user_id))]
    pub async fn refresh_session(
        &self,
        connection_id: &str,
        user_id: &str,
        session_id: &str,
    ) -> Result<()> {
        self.session_domain_service
            .refresh_heartbeat(user_id, session_id, Some(connection_id))
            .await
    }
}
