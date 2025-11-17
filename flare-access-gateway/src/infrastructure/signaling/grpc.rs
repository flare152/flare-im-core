use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use flare_proto::signaling::signaling_service_client::SignalingServiceClient;
use flare_proto::signaling::{
    GetOnlineStatusRequest, GetOnlineStatusResponse, HeartbeatRequest, HeartbeatResponse,
    LoginRequest, LoginResponse, LogoutRequest, LogoutResponse,
};
use flare_server_core::error::{ErrorBuilder, ErrorCode, InfraResult, InfraResultExt, Result};
use tokio::sync::Mutex;
use tonic::transport::Channel;

use crate::domain::SignalingGateway;

pub struct GrpcSignalingGateway {
    endpoint: String,
    client: Mutex<Option<SignalingServiceClient<Channel>>>,
}

impl GrpcSignalingGateway {
    pub fn new(endpoint: String) -> Self {
        Self {
            endpoint,
            client: Mutex::new(None),
        }
    }

    async fn ensure_client(&self) -> InfraResult<SignalingServiceClient<Channel>> {
        let mut guard = self.client.lock().await;
        if let Some(client) = guard.as_ref() {
            return Ok(client.clone());
        }

        let client = SignalingServiceClient::connect(self.endpoint.clone())
            .await
            .context("failed to connect signaling service")?;
        *guard = Some(client.clone());
        Ok(client)
    }
}

#[async_trait]
impl SignalingGateway for GrpcSignalingGateway {
    async fn login(&self, request: LoginRequest) -> Result<LoginResponse> {
        let mut client = self.ensure_client().await.into_flare(
            ErrorCode::ServiceUnavailable,
            "failed to connect signaling service",
        )?;
        client
            .login(request)
            .await
            .map(|resp| resp.into_inner())
            .map_err(|status| {
                ErrorBuilder::new(ErrorCode::ServiceUnavailable, "signaling login failed")
                    .details(status.to_string())
                    .build_error()
            })
    }

    async fn logout(&self, request: LogoutRequest) -> Result<LogoutResponse> {
        let mut client = self.ensure_client().await.into_flare(
            ErrorCode::ServiceUnavailable,
            "failed to connect signaling service",
        )?;
        client
            .logout(request)
            .await
            .map(|resp| resp.into_inner())
            .map_err(|status| {
                ErrorBuilder::new(ErrorCode::ServiceUnavailable, "signaling logout failed")
                    .details(status.to_string())
                    .build_error()
            })
    }

    async fn heartbeat(&self, request: HeartbeatRequest) -> Result<HeartbeatResponse> {
        let mut client = self.ensure_client().await.into_flare(
            ErrorCode::ServiceUnavailable,
            "failed to connect signaling service",
        )?;
        client
            .heartbeat(request)
            .await
            .map(|resp| resp.into_inner())
            .map_err(|status| {
                ErrorBuilder::new(ErrorCode::ServiceUnavailable, "signaling heartbeat failed")
                    .details(status.to_string())
                    .build_error()
            })
    }

    async fn get_online_status(
        &self,
        request: GetOnlineStatusRequest,
    ) -> Result<GetOnlineStatusResponse> {
        let mut client = self.ensure_client().await.into_flare(
            ErrorCode::ServiceUnavailable,
            "failed to connect signaling service",
        )?;
        client
            .get_online_status(request)
            .await
            .map(|resp| resp.into_inner())
            .map_err(|status| {
                ErrorBuilder::new(
                    ErrorCode::ServiceUnavailable,
                    "signaling get_online_status failed",
                )
                .details(status.to_string())
                .build_error()
            })
    }
}
