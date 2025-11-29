use std::collections::HashMap;

use flare_proto::signaling::{
    GetOnlineStatusRequest, GetOnlineStatusResponse, HeartbeatRequest, HeartbeatResponse,
    LoginRequest, LoginResponse, LogoutRequest, LogoutResponse,
};
use async_trait::async_trait;
use flare_server_core::error::Result;

use super::model::{ConnectionInfo, Session};


#[async_trait]
pub trait SessionStore: Send + Sync {
    async fn insert(&self, session: Session) -> Result<()>;
    async fn remove(&self, session_id: &str) -> Result<Option<Session>>;
    async fn update_connection(
        &self,
        session_id: &str,
        connection_id: Option<String>,
    ) -> Result<()>;
    async fn touch(&self, session_id: &str) -> Result<Option<Session>>;
    async fn find_by_user(&self, user_id: &str) -> Result<Vec<Session>>;
    async fn all(&self) -> Result<HashMap<String, Session>>;
}


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

#[async_trait]
pub trait ConnectionQuery: Send + Sync {
    /// 查询用户的所有连接
    async fn query_user_connections(&self, user_id: &str) -> Result<Vec<ConnectionInfo>>;
}
