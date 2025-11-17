//! Signaling Online客户端（用于批量查询在线状态）

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use flare_proto::signaling::signaling_service_client::SignalingServiceClient;
use flare_proto::signaling::{GetOnlineStatusRequest, GetOnlineStatusResponse};
use flare_server_core::error::{ErrorBuilder, ErrorCode, Result};
use tokio::sync::Mutex;
use tonic::transport::Channel;

use crate::domain::repositories::{OnlineStatus, OnlineStatusRepository};

/// Signaling Online客户端
pub struct SignalingOnlineClient {
    endpoint: String,
    client: Mutex<Option<SignalingServiceClient<Channel>>>,
}

impl SignalingOnlineClient {
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

    /// 批量查询用户在线状态
    pub async fn batch_get_online_status(
        &self,
        user_ids: &[String],
        tenant_id: Option<&str>,
    ) -> Result<HashMap<String, OnlineStatus>> {
        let mut client = self.ensure_client().await?;

        let request = GetOnlineStatusRequest {
            user_ids: user_ids.to_vec(),
            context: Some(flare_proto::RequestContext::default()),
            tenant: tenant_id.map(|tid| flare_proto::TenantContext {
                tenant_id: tid.to_string(),
                business_type: String::new(),
                environment: String::new(),
                organization_id: String::new(),
                labels: std::collections::HashMap::new(),
                attributes: std::collections::HashMap::new(),
            }),
        };

        let response: GetOnlineStatusResponse = client
            .get_online_status(tonic::Request::new(request))
            .await
            .map_err(|status| {
                ErrorBuilder::new(ErrorCode::ServiceUnavailable, "signaling query failed")
                    .details(status.to_string())
                    .build_error()
            })?
            .into_inner();

        // 转换为OnlineStatus映射
        let mut result = HashMap::new();
        for (user_id, status) in response.statuses {
            let gateway_id = if !status.server_id.is_empty() {
                Some(status.server_id.clone())
            } else if !status.cluster_id.is_empty() {
                Some(status.cluster_id.clone())
            } else {
                None
            };
            
            let server_id = if !status.server_id.is_empty() {
                Some(status.server_id.clone())
            } else {
                None
            };
            
            result.insert(
                user_id.clone(),
                OnlineStatus {
                    user_id,
                    online: status.online,
                    gateway_id,
                    server_id,
                },
            );
        }

        Ok(result)
    }
}

