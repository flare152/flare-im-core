use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use flare_proto::signaling::signaling_service_client::SignalingServiceClient;
use flare_proto::signaling::{
    GetOnlineStatusRequest, GetOnlineStatusResponse, LoginRequest, LoginResponse, LogoutRequest,
    LogoutResponse, RouteMessageRequest, RouteMessageResponse,
};
use flare_server_core::error::{ErrorBuilder, ErrorCode, Result};
use tokio::sync::Mutex;
use tonic::transport::Channel;

#[async_trait]
pub trait SignalingClient: Send + Sync {
    async fn login(&self, request: LoginRequest) -> Result<LoginResponse>;
    async fn logout(&self, request: LogoutRequest) -> Result<LogoutResponse>;
    async fn get_online_status(
        &self,
        request: GetOnlineStatusRequest,
    ) -> Result<GetOnlineStatusResponse>;
    async fn route_message(&self, request: RouteMessageRequest)
        -> Result<RouteMessageResponse>;
}

pub struct GrpcSignalingClient {
    endpoint: String,
    client: Mutex<Option<SignalingServiceClient<Channel>>>,
}

impl GrpcSignalingClient {
    pub fn new(endpoint: String) -> Arc<Self> {
        Arc::new(Self {
            endpoint,
            client: Mutex::new(None),
        })
    }

    async fn ensure_client(&self) -> Result<SignalingServiceClient<Channel>> {
        let mut guard = self.client.lock().await;
        if let Some(client) = guard.as_ref() {
            return Ok(client.clone());
        }

        let client = SignalingServiceClient::connect(self.endpoint.clone())
            .await
            .context("failed to connect signaling service")
            .map_err(|err| {
                ErrorBuilder::new(ErrorCode::ServiceUnavailable, "signaling unavailable")
                    .details(err.to_string())
                    .build_error()
            })?;
        *guard = Some(client.clone());
        Ok(client)
    }
}

#[async_trait]
impl SignalingClient for GrpcSignalingClient {
    async fn login(&self, request: LoginRequest) -> Result<LoginResponse> {
        let mut client = self.ensure_client().await?;
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
        let mut client = self.ensure_client().await?;

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

    async fn get_online_status(
        &self,
        request: GetOnlineStatusRequest,
    ) -> Result<GetOnlineStatusResponse> {
        let mut client = self.ensure_client().await?;

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

    async fn route_message(
        &self,
        request: RouteMessageRequest,
    ) -> Result<RouteMessageResponse> {
        let mut client = self.ensure_client().await?;
        client
            .route_message(request)
            .await
            .map(|resp| resp.into_inner())
            .map_err(|status| {
                ErrorBuilder::new(ErrorCode::ServiceUnavailable, "signaling route failed")
                    .details(status.to_string())
                    .build_error()
            })
    }
}

