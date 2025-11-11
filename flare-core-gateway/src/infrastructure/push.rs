use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use flare_proto::push::push_service_client::PushServiceClient;
use flare_proto::push::{
    PushMessageRequest, PushMessageResponse, PushNotificationRequest, PushNotificationResponse,
};
use flare_server_core::error::{ErrorBuilder, ErrorCode, Result};
use tokio::sync::Mutex;
use tonic::transport::Channel;

#[async_trait]
pub trait PushClient: Send + Sync {
    async fn push_message(&self, request: PushMessageRequest) -> Result<PushMessageResponse>;
    async fn push_notification(
        &self,
        request: PushNotificationRequest,
    ) -> Result<PushNotificationResponse>;
}

pub struct GrpcPushClient {
    endpoint: String,
    client: Mutex<Option<PushServiceClient<Channel>>>,
}

impl GrpcPushClient {
    pub fn new(endpoint: String) -> Arc<Self> {
        Arc::new(Self {
            endpoint,
            client: Mutex::new(None),
        })
    }

    async fn ensure_client(&self) -> Result<PushServiceClient<Channel>> {
        let mut guard = self.client.lock().await;
        if let Some(client) = guard.as_ref() {
            return Ok(client.clone());
        }

        let client = PushServiceClient::connect(self.endpoint.clone())
            .await
            .context("failed to connect push service")
            .map_err(|err| {
                ErrorBuilder::new(ErrorCode::ServiceUnavailable, "push service unavailable")
                    .details(err.to_string())
                    .build_error()
            })?;
        *guard = Some(client.clone());
        Ok(client)
    }
}

#[async_trait]
impl PushClient for GrpcPushClient {
    async fn push_message(&self, request: PushMessageRequest) -> Result<PushMessageResponse> {
        let mut client = self.ensure_client().await?;
        client
            .push_message(request)
            .await
            .map(|resp| resp.into_inner())
            .map_err(|status| {
                ErrorBuilder::new(ErrorCode::ServiceUnavailable, "push message failed")
                    .details(status.to_string())
                    .build_error()
            })
    }

    async fn push_notification(
        &self,
        request: PushNotificationRequest,
    ) -> Result<PushNotificationResponse> {
        let mut client = self.ensure_client().await?;
        client
            .push_notification(request)
            .await
            .map(|resp| resp.into_inner())
            .map_err(|status| {
                ErrorBuilder::new(ErrorCode::ServiceUnavailable, "push notification failed")
                    .details(status.to_string())
                    .build_error()
            })
    }
}

