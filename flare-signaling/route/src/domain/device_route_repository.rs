use anyhow::Result;
use async_trait::async_trait;

use crate::domain::entities::device_route::DeviceRoute;

/// 设备路由读模型仓储接口
#[async_trait]
pub trait DeviceRouteRepository: Send + Sync {
    /// 保存或更新某个用户某设备的路由
    async fn upsert(&self, route: DeviceRoute) -> Result<()>;

    /// 删除某个用户某设备的路由
    async fn remove(&self, user_id: &str, device_id: &str) -> Result<()>;

    /// 删除某个用户的所有设备路由
    async fn remove_all(&self, user_id: &str) -> Result<()>;

    /// 获取某个用户的所有设备路由
    async fn list_by_user(&self, user_id: &str) -> Result<Vec<DeviceRoute>>;
}
