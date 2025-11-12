use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use flare_proto::storage::storage_service_client::StorageServiceClient;
use flare_proto::storage::{
    BatchStoreMessageRequest, BatchStoreMessageResponse, QueryMessagesRequest,
    QueryMessagesResponse, StoreMessageRequest, StoreMessageResponse,
};
use flare_server_core::error::{ErrorBuilder, ErrorCode, Result};
use tokio::sync::Mutex;
use tonic::transport::Channel;

#[async_trait]
pub trait StorageClient: Send + Sync {
    async fn store_message(&self, request: StoreMessageRequest) -> Result<StoreMessageResponse>;
    async fn batch_store_message(
        &self,
        request: BatchStoreMessageRequest,
    ) -> Result<BatchStoreMessageResponse>;
    async fn query_messages(&self, request: QueryMessagesRequest) -> Result<QueryMessagesResponse>;
}

pub struct GrpcStorageClient {
    endpoint: String,
    client: Mutex<Option<StorageServiceClient<Channel>>>,
}

impl GrpcStorageClient {
    pub fn new(endpoint: String) -> Arc<Self> {
        Arc::new(Self {
            endpoint,
            client: Mutex::new(None),
        })
    }

    async fn ensure_client(&self) -> Result<StorageServiceClient<Channel>> {
        let mut guard = self.client.lock().await;
        if let Some(client) = guard.as_ref() {
            return Ok(client.clone());
        }

        let client = StorageServiceClient::connect(self.endpoint.clone())
            .await
            .context("failed to connect storage service")
            .map_err(|err| {
                ErrorBuilder::new(ErrorCode::ServiceUnavailable, "storage service unavailable")
                    .details(err.to_string())
                    .build_error()
            })?;
        *guard = Some(client.clone());
        Ok(client)
    }
}

#[async_trait]
impl StorageClient for GrpcStorageClient {
    async fn store_message(&self, request: StoreMessageRequest) -> Result<StoreMessageResponse> {
        let mut client = self.ensure_client().await?;
        client
            .store_message(request)
            .await
            .map(|resp| resp.into_inner())
            .map_err(|status| {
                ErrorBuilder::new(ErrorCode::ServiceUnavailable, "store message failed")
                    .details(status.to_string())
                    .build_error()
            })
    }

    async fn batch_store_message(
        &self,
        request: BatchStoreMessageRequest,
    ) -> Result<BatchStoreMessageResponse> {
        let mut client = self.ensure_client().await?;
        client
            .batch_store_message(request)
            .await
            .map(|resp| resp.into_inner())
            .map_err(|status| {
                ErrorBuilder::new(ErrorCode::ServiceUnavailable, "batch store message failed")
                    .details(status.to_string())
                    .build_error()
            })
    }

    async fn query_messages(&self, request: QueryMessagesRequest) -> Result<QueryMessagesResponse> {
        let mut client = self.ensure_client().await?;
        client
            .query_messages(request)
            .await
            .map(|resp| resp.into_inner())
            .map_err(|status| {
                ErrorBuilder::new(ErrorCode::ServiceUnavailable, "query messages failed")
                    .details(status.to_string())
                    .build_error()
            })
    }
}
