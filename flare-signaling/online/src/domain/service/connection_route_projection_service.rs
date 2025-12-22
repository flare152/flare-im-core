//! Online -> Route 设备路由同步领域服务
//!
//! 职责：
//! - 监听 Connection 聚合根产生的领域事件
//! - 将会话状态投影到 Route 模块的 DeviceRoute 读模型

use std::sync::Arc;

use anyhow::Result;
use tracing::debug;

use crate::domain::aggregate::Connection;
use crate::domain::event::{ConnectionCreatedEvent, QualityChangedEvent, ConnectionKickedEvent};
use flare_signaling_route::domain::DeviceRoute;
use flare_signaling_route::domain::DeviceRouteRepository;

pub struct ConnectionRouteProjectionService {
    device_route_repo: Arc<dyn DeviceRouteRepository>,
}

impl ConnectionRouteProjectionService {
    pub fn new(device_route_repo: Arc<dyn DeviceRouteRepository>) -> Self {
        Self { device_route_repo }
    }

    /// 当新 Connection 被创建时，将其投影为 DeviceRoute
    pub async fn on_session_created(&self, event: &ConnectionCreatedEvent, session: &Connection) -> Result<()> {
        let quality_score = session.quality_score();

        let route = DeviceRoute::new(
            event.user_id.as_str().to_string(),
            event.device_id.as_str().to_string(),
            session.gateway_id().to_string(),
            session.server_id().to_string(),
            event.device_priority.as_i32(),
            quality_score,
        );

        debug!(
            user_id = %event.user_id.as_str(),
            device_id = %event.device_id.as_str(),
            gateway_id = %session.gateway_id(),
            server_id = %session.server_id(),
            quality_score = quality_score,
            "projecting session_created to device_route",
        );

        self.device_route_repo.upsert(route).await
    }

    /// 当会话质量发生变化时，更新对应 DeviceRoute 的 quality_score
    pub async fn on_quality_changed(&self, event: &QualityChangedEvent, session: &Connection) -> Result<()> {
        let quality_score = session.quality_score();

        let route = DeviceRoute::new(
            event.user_id.as_str().to_string(),
            event.device_id.as_str().to_string(),
            session.gateway_id().to_string(),
            session.server_id().to_string(),
            session.device_priority().as_i32(),
            quality_score,
        );

        debug!(
            user_id = %event.user_id.as_str(),
            device_id = %event.device_id.as_str(),
            quality_score = quality_score,
            "projecting quality_changed to device_route",
        );

        self.device_route_repo.upsert(route).await
    }

    /// 当会话被踢下线时，删除对应 DeviceRoute
    pub async fn on_session_kicked(&self, event: &ConnectionKickedEvent) -> Result<()> {
        debug!(
            user_id = %event.user_id.as_str(),
            device_id = %event.device_id.as_str(),
            reason = %event.reason,
            "projecting session_kicked to device_route(remove)",
        );

        self.device_route_repo
            .remove(event.user_id.as_str(), event.device_id.as_str())
            .await
    }
}
