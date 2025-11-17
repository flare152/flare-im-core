//! 消息路由模块
//!
//! 负责将客户端消息路由到 Message Orchestrator

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use flare_proto::common::{RequestContext, TenantContext};
use flare_proto::message::message_service_client::MessageServiceClient;
use flare_proto::message::{SendMessageRequest, SendMessageResponse};
use flare_proto::storage::{Message, MessageType, MessageContent, TextContent};
use prost_types::Timestamp;
use tokio::sync::Mutex;
use tonic::transport::Channel;
use tracing::{error, info, warn};

/// 消息路由服务
pub struct MessageRouter {
    client: Arc<Mutex<Option<MessageServiceClient<Channel>>>>,
    endpoint: String,
    default_tenant_id: String,
}

impl MessageRouter {
    /// 创建新的消息路由服务
    pub fn new(endpoint: String, default_tenant_id: String) -> Self {
        Self {
            client: Arc::new(Mutex::new(None)),
            endpoint,
            default_tenant_id,
        }
    }

    /// 初始化客户端连接
    pub async fn initialize(&self) -> Result<()> {
        let endpoint = self.endpoint.clone();
        let channel = Channel::from_shared(endpoint.clone())
            .map_err(|e| anyhow::anyhow!("Invalid endpoint {}: {}", endpoint, e))?
            .connect()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to connect to Message Orchestrator at {}: {}", endpoint, e))?;

        let client = MessageServiceClient::new(channel);
        *self.client.lock().await = Some(client);
        
        info!(endpoint = %self.endpoint, "Message Router initialized");
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
        let mut client_guard = self.client.lock().await;
        
        // 如果客户端未初始化，尝试初始化
        if client_guard.is_none() {
            warn!("Message Router client not initialized, attempting to initialize...");
            drop(client_guard);
            self.initialize().await?;
            client_guard = self.client.lock().await;
        }

        let client = client_guard.as_mut()
            .ok_or_else(|| anyhow::anyhow!("Message Router client not available"))?;

        // 构建消息内容
        let text_content = String::from_utf8_lossy(&payload).to_string();
        let message_content = MessageContent {
            content: Some(flare_proto::storage::message_content::Content::Text(
                TextContent {
                    text: text_content,
                    mentions: vec![], // @提及列表（聊天室消息暂时为空）
                },
            )),
        };

        // 构建消息
        let message = Message {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            sender_id: user_id.to_string(),
            sender_type: "user".to_string(),
            receiver_ids: vec![], // 聊天室消息，接收者为空（广播）
            receiver_id: String::new(),
            content: Some(message_content),
            timestamp: Some(Timestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: 0,
            }),
            message_type: 1, // MessageType::Text = 1
            business_type: "chatroom".to_string(),
            session_type: "group".to_string(), // 聊天室是群组类型
            status: String::new(),
            extra: {
                let mut extra = std::collections::HashMap::new();
                extra.insert("source".to_string(), "access_gateway".to_string());
                extra.insert("chatroom".to_string(), "true".to_string());
                if let Some(tenant_id) = tenant_id {
                    extra.insert("tenant_id".to_string(), tenant_id.to_string());
                }
                extra
            },
            is_recalled: false,
            recalled_at: None,
            is_burn_after_read: false,
            burn_after_seconds: 0,
            tenant: Some(TenantContext {
                tenant_id: tenant_id.unwrap_or(&self.default_tenant_id).to_string(),
                business_type: "chatroom".to_string(),
                environment: String::new(),
                organization_id: String::new(),
                labels: std::collections::HashMap::new(),
                attributes: std::collections::HashMap::new(),
            }),
            attachments: vec![],
            audit: None,
            tags: vec![],
            timeline: None,
            attributes: std::collections::HashMap::new(),
            read_by: vec![],
            visibility: std::collections::HashMap::new(),
            operations: vec![],
        };

        // 构建请求上下文
        let request_context = RequestContext {
            request_id: uuid::Uuid::new_v4().to_string(),
            trace: None,
            actor: Some(flare_proto::common::ActorContext {
                actor_id: user_id.to_string(),
                r#type: flare_proto::common::ActorType::User as i32, // ACTOR_TYPE_USER = 1
                roles: vec![],
                attributes: std::collections::HashMap::new(),
            }),
            device: None,
            channel: String::new(),
            user_agent: String::new(),
            attributes: std::collections::HashMap::new(),
        };

        // 构建发送请求
        let request = SendMessageRequest {
            session_id: session_id.to_string(),
            message: Some(message),
            sync: false, // 异步模式，立即返回
            context: Some(request_context),
            tenant: Some(TenantContext {
                tenant_id: tenant_id.unwrap_or(&self.default_tenant_id).to_string(),
                business_type: "chatroom".to_string(),
                environment: String::new(),
                organization_id: String::new(),
                labels: std::collections::HashMap::new(),
                attributes: std::collections::HashMap::new(),
            }),
        };

        // 发送请求
        let response = client
            .send_message(tonic::Request::new(request))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send message to Message Orchestrator: {}", e))?;

        let response = response.into_inner();
        
        info!(
            user_id = %user_id,
            session_id = %session_id,
            message_id = %response.message_id,
            "Message routed to Message Orchestrator"
        );

        Ok(response)
    }

    /// 检查客户端是否已连接
    pub async fn is_connected(&self) -> bool {
        self.client.lock().await.is_some()
    }
}

