use anyhow::Result;
use async_trait::async_trait;

use crate::domain::model::route::Route;

/// 路由仓储接口（需要作为 trait 对象使用，保留 async-trait）
#[async_trait]
pub trait RouteRepository: Send + Sync {
    /// 保存路由
    async fn save(&self, route: Route) -> Result<()>;

    /// 根据服务 ID 查找路由
    async fn find_by_svid(&self, svid: &str) -> Result<Option<Route>>;

    /// 删除路由
    async fn delete(&self, svid: &str) -> Result<()>;
}
