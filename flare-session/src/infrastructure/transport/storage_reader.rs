use anyhow::{Context, Result};
use async_trait::async_trait;
use flare_proto::common::TenantContext;
use flare_proto::storage::QueryMessagesRequest;
use flare_proto::storage::storage_reader_service_client::StorageReaderServiceClient;
use flare_server_core::discovery::ServiceClient;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::Request;
use tonic::transport::Channel;

use crate::domain::model::MessageSyncResult;
use crate::domain::repository::MessageProvider;

pub struct StorageReaderMessageProvider {
    service_name: String,
    service_client: Arc<Mutex<Option<ServiceClient>>>,
}

impl StorageReaderMessageProvider {
    /// 创建新的消息提供者（使用服务名称，内部创建服务发现）
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            service_client: Arc::new(Mutex::new(None)),
        }
    }

    /// 使用 ServiceClient 创建新的消息提供者（推荐，通过 wire 注入）
    pub fn with_service_client(service_client: ServiceClient) -> Self {
        Self {
            service_name: String::new(), // 不需要 service_name
            service_client: Arc::new(Mutex::new(Some(service_client))),
        }
    }

    async fn client(&self) -> Result<StorageReaderServiceClient<Channel>> {
        // 使用服务发现获取 Channel
        let mut service_client_guard = self.service_client.lock().await;
        if service_client_guard.is_none() {
            if self.service_name.is_empty() {
                return Err(anyhow::anyhow!("storage_reader_service is not configured"));
            }
            
            // 如果没有注入 ServiceClient，则创建服务发现器
            let discover = flare_im_core::discovery::create_discover(&self.service_name)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to create service discover: {}", e))?;
            
            if let Some(discover) = discover {
                *service_client_guard = Some(ServiceClient::new(discover));
            } else {
                return Err(anyhow::anyhow!("Service discovery not configured"));
            }
        }
        
        let service_client = service_client_guard.as_mut().unwrap();
        let channel = service_client.get_channel().await
            .map_err(|e| anyhow::anyhow!("Failed to get channel from service discovery: {}", e))?;
        
        tracing::debug!("Got channel for storage reader service from service discovery");
        
        Ok(StorageReaderServiceClient::new(channel))
    }

    fn last_timestamp(messages: &[flare_proto::common::Message]) -> Option<i64> {
        messages
            .last()
            .and_then(|msg| msg.timestamp.as_ref())
            .map(|ts| ts.seconds * 1_000 + (ts.nanos as i64 / 1_000_000))
    }

    fn build_request(
        session_id: &str,
        since_ts: i64,
        cursor: Option<&str>,
        limit: i32,
    ) -> QueryMessagesRequest {
        QueryMessagesRequest {
            session_id: session_id.to_string(),
            start_time: since_ts,
            end_time: 0,
            limit,
            cursor: cursor.unwrap_or_default().to_string(),
            context: None,
            tenant: Some(TenantContext {
                tenant_id: String::new(),
                ..Default::default()
            }),
            pagination: None,
        }
    }

    fn map_response(resp: flare_proto::storage::QueryMessagesResponse) -> MessageSyncResult {
        let server_cursor_ts = Self::last_timestamp(&resp.messages);
        MessageSyncResult {
            messages: resp.messages,
            next_cursor: if resp.next_cursor.is_empty() {
                None
            } else {
                Some(resp.next_cursor)
            },
            server_cursor_ts,
        }
    }
}

#[async_trait]
impl MessageProvider for StorageReaderMessageProvider {
    async fn sync_messages(
        &self,
        session_id: &str,
        since_ts: i64,
        cursor: Option<&str>,
        limit: i32,
    ) -> Result<MessageSyncResult> {
        let mut client = self.client().await?;
        let request = Self::build_request(session_id, since_ts, cursor, limit);
        let response = client
            .query_messages(Request::new(request))
            .await
            .context("call storage reader query_messages")?
            .into_inner();
        Ok(Self::map_response(response))
    }

    async fn recent_messages(
        &self,
        session_ids: &[String],
        limit_per_session: i32,
        client_cursor: &HashMap<String, i64>,
    ) -> Result<Vec<flare_proto::common::Message>> {
        let mut messages = Vec::new();
        let mut client = self.client().await?;
        for session_id in session_ids {
            let since_ts = client_cursor.get(session_id).copied().unwrap_or(0);
            let request = Self::build_request(session_id, since_ts, None, limit_per_session);
            let response = client
                .query_messages(Request::new(request))
                .await
                .with_context(|| format!("fetch recent messages for {}", session_id))?
                .into_inner();
            messages.extend(response.messages);
        }
        Ok(messages)
    }
}
