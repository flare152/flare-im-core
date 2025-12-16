use serde::{Deserialize, Serialize};

/// 解析路由查询
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveRouteQuery {
    /// 服务 ID
    pub svid: String,
}
