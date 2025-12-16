//! 消息路由模块
//!
//! 负责将客户端消息路由到 Message Orchestrator
//! 支持两种模式：
//! 1. 直接路由模式：直接路由到 message-orchestrator（向后兼容）
//! 2. Route 服务模式：通过 Route 服务进行 SVID 路由（推荐）

use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::Utc;
use flare_proto::common::{RequestContext, TenantContext};
use flare_proto::message::message_service_client::MessageServiceClient;
use flare_proto::message::{SendMessageRequest, SendMessageResponse};
use prost::Message as ProstMessage;
use prost_types::Timestamp;
use tokio::sync::Mutex;
use tonic::transport::Channel;
use tracing::{error, info, warn};

use flare_server_core::discovery::ServiceClient;

/// 消息路由服务
pub struct MessageRouter {
    service_name: String,
    default_tenant_id: String,
    /// gRPC 客户端
    client: Arc<Mutex<Option<MessageServiceClient<Channel>>>>,
    service_client: Arc<Mutex<Option<ServiceClient>>>,
    /// 默认 SVID
    default_svid: String,
}

impl MessageRouter {
    /// 创建新的消息路由服务（使用服务名称，内部创建服务发现）
    pub fn new(service_name: String, default_tenant_id: String, default_svid: String) -> Self {
        Self {
            client: Arc::new(Mutex::new(None)),
            service_name,
            default_tenant_id,
            service_client: Arc::new(Mutex::new(None)),
            default_svid,
        }
    }

    /// 使用 ServiceClient 创建新的消息路由服务（推荐，通过 wire 注入）
    pub fn with_service_client(
        service_client: ServiceClient,
        default_tenant_id: String,
        default_svid: String,
    ) -> Self {
        Self {
            client: Arc::new(Mutex::new(None)),
            service_name: String::new(), // 不需要 service_name
            default_tenant_id,
            service_client: Arc::new(Mutex::new(Some(service_client))),
            default_svid,
        }
    }

    /// 初始化客户端连接
    pub async fn initialize(&self) -> Result<()> {
        use flare_server_core::discovery::ServiceClient;

        info!(
            service_name = %self.service_name,
            "Initializing Message Router client..."
        );

        // 使用服务发现获取 Channel
        let mut service_client_guard = self.service_client.lock().await;
        if service_client_guard.is_none() {
            // 如果没有注入 ServiceClient，则创建服务发现器
            let discover = flare_im_core::discovery::create_discover(&self.service_name)
                .await
                .map_err(|e| {
                    error!(
                        service_name = %self.service_name,
                        error = %e,
                        "Failed to create service discover for message orchestrator"
                    );
                    anyhow::anyhow!("Failed to create service discover: {}", e)
                })?;

            let discover =
                discover.ok_or_else(|| anyhow::anyhow!("Service discovery not configured"))?;

            *service_client_guard = Some(ServiceClient::new(discover));
        }

        let service_client = service_client_guard
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Service client not initialized"))?;
        let channel = service_client.get_channel().await.map_err(|e| {
            error!(
                service_name = %self.service_name,
                error = %e,
                "Failed to get channel from service discovery"
            );
            anyhow::anyhow!("Failed to get channel: {}", e)
        })?;

        info!(
            service_name = %self.service_name,
            "Successfully got channel from service discovery"
        );

        let client = MessageServiceClient::new(channel);
        *self.client.lock().await = Some(client);

        info!(
            service_name = %self.service_name,
            "✅ Message Router initialized successfully"
        );
        Ok(())
    }

    /// 路由消息到 Message Orchestrator
    pub async fn route_message(
        &self,
        user_id: &str,
        session_id: &str,
        payload: Vec<u8>,
        tenant_id: Option<&str>,
    ) -> Result<SendMessageResponse> {
        // 直接路由模式
        self.route_direct(user_id, session_id, payload, tenant_id)
            .await
    }

    /// 直接路由到 Message Orchestrator（向后兼容模式）
    async fn route_direct(
        &self,
        user_id: &str,
        session_id: &str,
        payload: Vec<u8>,
        tenant_id: Option<&str>,
    ) -> Result<SendMessageResponse> {
        // 确保客户端已初始化
        let mut client_guard = self.ensure_client().await?;

        let client = client_guard.as_mut().ok_or_else(|| {
            anyhow::anyhow!("Message Router client not available after initialization")
        })?;

        // 构建发送请求
        let request = self.build_send_message_request(user_id, session_id, payload, tenant_id)?;

        // 发送请求（添加超时保护，避免阻塞）
        let timeout_duration = std::time::Duration::from_secs(5); // 5秒超时
        let response = match tokio::time::timeout(
            timeout_duration,
            client.send_message(tonic::Request::new(request)),
        )
        .await
        {
            Ok(Ok(resp)) => resp,
            Ok(Err(e)) => {
                error!(
                    error = %e,
                    user_id = %user_id,
                    session_id = %session_id,
                    "Failed to send message to Message Orchestrator"
                );
                // 如果连接失败，清除客户端以便下次重试
                {
                    let mut client_guard = self.client.lock().await;
                    *client_guard = None;
                }
                return Err(anyhow::anyhow!(
                    "Failed to send message to Message Orchestrator: {}",
                    e
                ));
            }
            Err(_) => {
                error!(
                    user_id = %user_id,
                    session_id = %session_id,
                    timeout_secs = timeout_duration.as_secs(),
                    "Timeout sending message to Message Orchestrator"
                );
                return Err(anyhow::anyhow!(
                    "Timeout sending message to Message Orchestrator (timeout: {}s)",
                    timeout_duration.as_secs()
                ));
            }
        };

        let response = response.into_inner();

        info!(
            user_id = %user_id,
            session_id = %session_id,
            message_id = %response.message_id,
            "Message routed to Message Orchestrator successfully"
        );

        Ok(response)
    }

    /// 检查客户端是否已连接
    pub async fn is_connected(&self) -> bool {
        self.client.lock().await.is_some()
    }

    /// 确保客户端已初始化（内部辅助函数）
    ///
    /// 如果客户端未初始化，尝试初始化（最多重试3次）
    /// 返回客户端引用，如果初始化失败则返回错误
    async fn ensure_client(
        &self,
    ) -> Result<tokio::sync::MutexGuard<'_, Option<MessageServiceClient<Channel>>>> {
        let client_guard = self.client.lock().await;

        // 如果客户端已初始化，直接返回
        if client_guard.is_some() {
            return Ok(client_guard);
        }

        // 客户端未初始化，需要初始化
        warn!(
            service_name = %self.service_name,
            "Message Router client not initialized, attempting to initialize..."
        );
        drop(client_guard);

        // 重试初始化（最多3次）
        let mut retries = 3;
        let mut last_error = None;
        while retries > 0 {
            match self.initialize().await {
                Ok(_) => {
                    info!(
                        service_name = %self.service_name,
                        "✅ Message Router initialized successfully after retry"
                    );
                    break;
                }
                Err(e) => {
                    let error_msg = format!("{}", e);
                    last_error = Some(anyhow::anyhow!("{}", error_msg));
                    retries -= 1;
                    if retries > 0 {
                        warn!(
                            service_name = %self.service_name,
                            error = %error_msg,
                            retries_left = retries,
                            "Failed to initialize Message Router, retrying..."
                        );
                        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                    } else {
                        error!(
                            service_name = %self.service_name,
                            error = %error_msg,
                            "❌ Failed to initialize Message Router after all retries"
                        );
                    }
                }
            }
        }

        if let Some(ref err) = last_error {
            error!(
                service_name = %self.service_name,
                error = %err,
                "Cannot route message: Message Router initialization failed"
            );
            return Err(anyhow::anyhow!(
                "Failed to initialize Message Router after retries: {}",
                err
            ));
        }

        // 重新获取锁
        Ok(self.client.lock().await)
    }

    /// 构建 SendMessageRequest（内部辅助函数）
    ///
    /// 验证 payload 必须是 Message 对象，然后使用它构建请求
    fn build_send_message_request(
        &self,
        user_id: &str,
        session_id: &str,
        payload: Vec<u8>,
        tenant_id: Option<&str>,
    ) -> Result<SendMessageRequest> {
        // 验证 payload 必须是 Message 对象
        let mut message = flare_proto::Message::decode(&payload[..]).map_err(|e| {
            anyhow::anyhow!(
                "Payload must be a valid Message object, decode error: {}",
                e
            )
        })?;

        // 确保消息的基本字段正确（如果 payload 中的字段为空，使用传入的参数填充）
        if message.session_id.is_empty() {
            message.session_id = session_id.to_string();
        }
        if message.sender_id.is_empty() {
            message.sender_id = user_id.to_string();
        }
        if message.id.is_empty() {
            message.id = uuid::Uuid::new_v4().to_string();
        }

        // 使用解析后的 Message 对象，确保时间戳等字段正确
        if message.timestamp.is_none() {
            message.timestamp = Some(Timestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: 0,
            });
        }

        // 在 extra 中添加来源标识
        message
            .extra
            .insert("source".to_string(), "access_gateway".to_string());
        if let Some(tenant_id) = tenant_id {
            message
                .extra
                .insert("tenant_id".to_string(), tenant_id.to_string());
        }

        // 构建请求上下文
        let request_context = self.build_request_context(user_id);

        // 构建发送请求
        Ok(SendMessageRequest {
            session_id: session_id.to_string(),
            message: Some(message),
            sync: false, // 异步模式，立即返回
            context: Some(request_context),
            tenant: Some(self.build_tenant_context(tenant_id)),
        })
    }

    /// 构建 RequestContext（内部辅助函数）
    fn build_request_context(&self, actor_id: &str) -> RequestContext {
        RequestContext {
            request_id: uuid::Uuid::new_v4().to_string(),
            trace: None,
            actor: Some(flare_proto::common::ActorContext {
                actor_id: actor_id.to_string(),
                r#type: flare_proto::common::ActorType::User as i32, // ACTOR_TYPE_USER = 1
                roles: vec![],
                attributes: std::collections::HashMap::new(),
            }),
            device: None,
            channel: String::new(),
            user_agent: String::new(),
            attributes: std::collections::HashMap::new(),
        }
    }

    /// 构建 TenantContext（内部辅助函数）
    fn build_tenant_context(&self, tenant_id: Option<&str>) -> TenantContext {
        TenantContext {
            tenant_id: tenant_id.unwrap_or(&self.default_tenant_id).to_string(),
            business_type: "chatroom".to_string(),
            environment: String::new(),
            organization_id: String::new(),
            labels: std::collections::HashMap::new(),
            attributes: std::collections::HashMap::new(),
        }
    }
}
