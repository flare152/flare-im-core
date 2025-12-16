use async_trait::async_trait;
use flare_proto::signaling::{
    GetOnlineStatusRequest, GetOnlineStatusResponse, HeartbeatRequest, HeartbeatResponse,
    LoginRequest, LoginResponse, LogoutRequest, LogoutResponse,
};
use flare_server_core::error::Result;

use super::model::ConnectionInfo;

/// Signaling Gateway 接口
///
/// Gateway 通过此接口与 Signaling Online 服务交互
#[async_trait]
pub trait SignalingGateway: Send + Sync {
    async fn login(&self, request: LoginRequest) -> Result<LoginResponse>;
    async fn logout(&self, request: LogoutRequest) -> Result<LogoutResponse>;
    async fn heartbeat(&self, request: HeartbeatRequest) -> Result<HeartbeatResponse>;
    async fn get_online_status(
        &self,
        request: GetOnlineStatusRequest,
    ) -> Result<GetOnlineStatusResponse>;
}

/// 连接查询接口
///
/// Gateway 通过此接口查询本地连接状态
#[async_trait]
pub trait ConnectionQuery: Send + Sync {
    /// 查询用户的所有连接
    async fn query_user_connections(&self, user_id: &str) -> Result<Vec<ConnectionInfo>>;
}
