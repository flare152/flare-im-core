use anyhow::{Context, Result};
use flare_proto::common::TenantContext;
use flare_proto::storage::QueryMessagesRequest;
use flare_proto::storage::storage_reader_service_client::StorageReaderServiceClient;
use flare_server_core::discovery::ServiceClient;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::Request;
use tonic::transport::{Channel, Endpoint};

use crate::domain::model::MessageSyncResult;
use crate::domain::repository::MessageProvider;
use async_trait::async_trait;
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
                // Fallback: direct gRPC address via env STORAGE_READER_GRPC_ADDR
                let addr = std::env::var("STORAGE_READER_GRPC_ADDR")
                    .ok()
                    .unwrap_or_else(|| "127.0.0.1:60083".to_string());
                let endpoint = Endpoint::from_shared(format!("http://{}", addr))
                    .map_err(|e| anyhow::anyhow!("create endpoint: {}", e))?;
                let channel = endpoint
                    .connect()
                    .await
                    .map_err(|e| anyhow::anyhow!("connect storage reader: {}", e))?;
                tracing::warn!(address = %addr, "Using STORAGE_READER_GRPC_ADDR fallback for storage reader");
                return Ok(StorageReaderServiceClient::new(channel));
            }
        }

        let service_client = service_client_guard
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Service client not initialized"))?;
        let channel = service_client
            .get_channel()
            .await
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

    fn last_seq(messages: &[flare_proto::common::Message]) -> Option<i64> {
        messages
            .last()
            .and_then(|msg| flare_im_core::utils::extract_seq_from_message(msg))
    }

    fn map_response(resp: flare_proto::storage::QueryMessagesResponse) -> MessageSyncResult {
        let server_cursor_ts = Self::last_timestamp(&resp.messages);
        let server_cursor_seq = Self::last_seq(&resp.messages);
        MessageSyncResult {
            messages: resp.messages,
            next_cursor: if resp.next_cursor.is_empty() {
                None
            } else {
                Some(resp.next_cursor)
            },
            server_cursor_ts,
            server_cursor_seq,
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
        // 优化：并行查询多个会话的消息，提高性能
        // 注意：gRPC client 不能跨任务共享，每个任务需要重新获取 client
        use tokio::task::JoinSet;

        let mut join_set = JoinSet::new();
        let service_name = self.service_name.clone();
        let service_client = Arc::clone(&self.service_client);

        // 为每个会话创建查询任务
        for session_id in session_ids {
            let session_id = session_id.clone();
            let since_ts = client_cursor.get(&session_id).copied().unwrap_or(0);
            let limit = limit_per_session;
            let service_name = service_name.clone();
            let service_client = Arc::clone(&service_client);

            join_set.spawn(async move {
                // 每个任务重新获取 client（因为 gRPC client 不能跨任务共享）
                // 直接调用 client() 方法的逻辑
                let mut service_client_guard = service_client.lock().await;
                if service_client_guard.is_none() {
                    if !service_name.is_empty() {
                        let discover = flare_im_core::discovery::create_discover(&service_name)
                            .await
                            .map_err(|e| {
                                anyhow::anyhow!("Failed to create service discover: {}", e)
                            })?;

                        if let Some(discover) = discover {
                            *service_client_guard = Some(ServiceClient::new(discover));
                        }
                    }
                }

                let channel: Channel = if let Some(service_client) = service_client_guard.as_mut() {
                    match service_client.get_channel().await {
                        Ok(ch) => ch,
                        Err(_e) => {
                            let addr = std::env::var("STORAGE_READER_GRPC_ADDR")
                                .ok()
                                .unwrap_or_else(|| "127.0.0.1:50091".to_string());
                            let endpoint = Endpoint::from_shared(format!("http://{}", addr))
                                .map_err(|err| anyhow::anyhow!("Invalid endpoint: {}", err))?;
                            endpoint
                                .connect()
                                .await
                                .map_err(|err| anyhow::anyhow!("Failed to connect: {}", err))?
                        }
                    }
                } else {
                    let addr = std::env::var("STORAGE_READER_GRPC_ADDR")
                        .ok()
                        .unwrap_or_else(|| "127.0.0.1:50091".to_string());
                    let endpoint = Endpoint::from_shared(format!("http://{}", addr))
                        .map_err(|err| anyhow::anyhow!("Invalid endpoint: {}", err))?;
                    endpoint
                        .connect()
                        .await
                        .map_err(|err| anyhow::anyhow!("Failed to connect: {}", err))?
                };

                let mut client = StorageReaderServiceClient::new(channel);
                let request = Self::build_request(&session_id, since_ts, None, limit);
                let response = client
                    .query_messages(Request::new(request))
                    .await
                    .with_context(|| format!("fetch recent messages for {}", session_id))?
                    .into_inner();
                Ok::<Vec<flare_proto::common::Message>, anyhow::Error>(response.messages)
            });
        }

        // 收集所有查询结果
        let mut messages = Vec::new();
        while let Some(result) = join_set.join_next().await {
            match result {
                Ok(Ok(msgs)) => {
                    messages.extend(msgs);
                }
                Ok(Err(e)) => {
                    // 单个会话查询失败，记录警告但不中断整个流程
                    tracing::warn!(error = %e, "Failed to fetch recent messages for one session");
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Task join error while fetching recent messages");
                }
            }
        }

        // 按时间戳排序（最新的在前）
        messages.sort_by(|a, b| {
            let a_ts = a
                .timestamp
                .as_ref()
                .map(|ts| ts.seconds * 1_000_000_000 + ts.nanos as i64)
                .unwrap_or(0);
            let b_ts = b
                .timestamp
                .as_ref()
                .map(|ts| ts.seconds * 1_000_000_000 + ts.nanos as i64)
                .unwrap_or(0);
            b_ts.cmp(&a_ts)
        });

        Ok(messages)
    }

    async fn sync_messages_by_seq(
        &self,
        session_id: &str,
        after_seq: i64,
        before_seq: Option<i64>,
        limit: i32,
    ) -> Result<MessageSyncResult> {
        let mut client = self.client().await?;
        let request = flare_proto::storage::QueryMessagesBySeqRequest {
            session_id: session_id.to_string(),
            after_seq,
            before_seq: before_seq.unwrap_or(0),
            limit,
            user_id: String::new(), // 可选，用于过滤已删除消息
            context: None,
            tenant: Some(TenantContext {
                tenant_id: String::new(),
                ..Default::default()
            }),
        };

        let response = client
            .query_messages_by_seq(Request::new(request))
            .await
            .context("call storage reader query_messages_by_seq")?
            .into_inner();

        // 构建 MessageSyncResult
        let server_cursor_ts = Self::last_timestamp(&response.messages);
        let server_cursor_seq = if response.last_seq > 0 {
            Some(response.last_seq)
        } else {
            None
        };

        Ok(MessageSyncResult {
            messages: response.messages,
            next_cursor: if response.next_cursor.is_empty() {
                None
            } else {
                Some(response.next_cursor)
            },
            server_cursor_ts,
            server_cursor_seq,
        })
    }
}
