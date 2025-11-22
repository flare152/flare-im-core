//! 查询结构体定义（Query DTO）

use flare_proto::signaling::GetOnlineStatusRequest;

/// 查询在线状态查询
#[derive(Debug, Clone)]
pub struct GetOnlineStatusQuery {
    /// 原始请求
    pub request: GetOnlineStatusRequest,
}





