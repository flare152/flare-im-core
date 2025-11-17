use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
// Note: Storage service client is not directly used here
// Storage operations are handled through Message Orchestrator
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
    // Note: Storage operations are handled through Message Orchestrator
    // This client is kept for backward compatibility but may not be fully implemented
}

impl GrpcStorageClient {
    pub fn new(endpoint: String) -> Arc<Self> {
        Arc::new(Self {
            endpoint,
        })
    }
}

#[async_trait]
impl StorageClient for GrpcStorageClient {
    async fn store_message(&self, _request: StoreMessageRequest) -> Result<StoreMessageResponse> {
        // Note: Storage operations should go through Message Orchestrator
        Err(ErrorBuilder::new(
            ErrorCode::ServiceUnavailable,
            "store_message should be called through Message Orchestrator",
        )
        .build_error())
    }

    async fn batch_store_message(
        &self,
        _request: BatchStoreMessageRequest,
    ) -> Result<BatchStoreMessageResponse> {
        // Note: Storage operations should go through Message Orchestrator
        Err(ErrorBuilder::new(
            ErrorCode::ServiceUnavailable,
            "batch_store_message should be called through Message Orchestrator",
        )
        .build_error())
    }

    async fn query_messages(&self, _request: QueryMessagesRequest) -> Result<QueryMessagesResponse> {
        // Note: Query operations should go through Storage Reader Service
        Err(ErrorBuilder::new(
            ErrorCode::ServiceUnavailable,
            "query_messages should be called through Storage Reader Service",
        )
        .build_error())
    }
}
