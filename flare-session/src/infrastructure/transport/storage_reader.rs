use anyhow::{Context, Result};
use async_trait::async_trait;
use flare_proto::common::TenantContext;
use flare_proto::storage::QueryMessagesRequest;
use flare_proto::storage::storage_reader_service_client::StorageReaderServiceClient;
use std::collections::HashMap;
use tonic::Request;
use tonic::transport::{Channel, Endpoint};

use crate::domain::models::MessageSyncResult;
use crate::domain::repositories::MessageProvider;

pub struct StorageReaderMessageProvider {
    endpoint: String,
}

impl StorageReaderMessageProvider {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
        }
    }

    async fn client(&self) -> Result<StorageReaderServiceClient<Channel>> {
        if self.endpoint.is_empty() {
            return Err(anyhow::anyhow!("storage_reader_endpoint is not configured"));
        }
        let channel = Endpoint::from_shared(self.endpoint.clone())
            .context("invalid storage_reader_endpoint URI")?
            .connect()
            .await
            .context("failed to connect to storage reader service")?;
        Ok(StorageReaderServiceClient::new(channel))
    }

    fn last_timestamp(messages: &[flare_proto::storage::Message]) -> Option<i64> {
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
    ) -> Result<Vec<flare_proto::storage::Message>> {
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
