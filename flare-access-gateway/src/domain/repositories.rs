use std::collections::HashMap;

use async_trait::async_trait;
use flare_proto::signaling::{
    GetOnlineStatusRequest, GetOnlineStatusResponse, HeartbeatRequest, HeartbeatResponse,
    LoginRequest, LoginResponse, LogoutRequest, LogoutResponse,
};
use flare_server_core::error::Result;

use super::Session;

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
