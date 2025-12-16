use serde::{Deserialize, Serialize};

/// 路由实体
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Route {
    /// 服务 ID
    pub svid: String,
    /// 端点地址
    pub endpoint: String,
}

impl Route {
    /// 创建新的路由
    pub fn new(svid: String, endpoint: String) -> Self {
        Self { svid, endpoint }
    }
}
