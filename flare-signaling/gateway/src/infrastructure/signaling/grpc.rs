use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use flare_proto::signaling::signaling_service_client::SignalingServiceClient;
use flare_proto::signaling::{
    GetOnlineStatusRequest, GetOnlineStatusResponse, HeartbeatRequest, HeartbeatResponse,
    LoginRequest, LoginResponse, LogoutRequest, LogoutResponse,
};
use flare_server_core::discovery::ServiceClient;
use flare_server_core::error::{ErrorBuilder, ErrorCode, InfraResult, InfraResultExt, Result};
use tokio::sync::Mutex;
use tonic::transport::Channel;

use crate::domain::repository::SignalingGateway;

pub struct GrpcSignalingGateway {
    service_name: String,
    service_client: Mutex<Option<ServiceClient>>,
    client: Mutex<Option<SignalingServiceClient<Channel>>>,
}

impl GrpcSignalingGateway {
    pub fn new(service_name: String) -> Self {
        Self {
            service_name,
            service_client: Mutex::new(None),
            client: Mutex::new(None),
        }
    }

    pub fn with_service_client(service_name: String, service_client: ServiceClient) -> Self {
        Self {
            service_name,
            service_client: Mutex::new(Some(service_client)),
            client: Mutex::new(None),
        }
    }

    async fn ensure_client(&self) -> InfraResult<SignalingServiceClient<Channel>> {
        let mut guard = self.client.lock().await;
        if let Some(client) = guard.as_ref() {
            return Ok(client.clone());
        }

        // 使用服务发现获取 Channel
        let mut service_client_guard = self.service_client.lock().await;
        if service_client_guard.is_none() {
            // 创建服务发现器
            let discover = flare_im_core::discovery::create_discover(&self.service_name)
                .await
                .map_err(|e| {
                    ErrorBuilder::new(
                        ErrorCode::ServiceUnavailable,
                        "signaling service unavailable",
                    )
                    .details(format!(
                        "Failed to create service discover for {}: {}",
                        self.service_name, e
                    ))
                    .build_error()
                })?;

            if let Some(discover) = discover {
                *service_client_guard = Some(ServiceClient::new(discover));
            } else {
                return Err(anyhow::anyhow!("Service discovery not configured").into());
            }
        }

        let service_client = service_client_guard
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Service client not initialized"))?;
        let channel = service_client.get_channel().await.map_err(|e| {
            ErrorBuilder::new(ErrorCode::ServiceUnavailable, "signaling unavailable")
                .details(format!("Failed to get channel: {}", e))
                .build_error()
        })?;

        tracing::debug!(
            "Got channel for {} from service discovery",
            self.service_name
        );

        let client = SignalingServiceClient::new(channel);
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
