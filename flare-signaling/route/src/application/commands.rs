use serde::{Deserialize, Serialize};

/// 注册路由命令
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterRouteCommand {
    /// 服务 ID
    pub svid: String,
    /// 端点地址
    pub endpoint: String,
}
