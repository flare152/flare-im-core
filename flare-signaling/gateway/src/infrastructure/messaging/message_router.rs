//! 消息路由模块
//!
//! 负责将客户端消息路由到 Message Orchestrator
//! 支持两种模式：
//! 1. 直接路由模式：直接路由到 message-orchestrator（向后兼容）
//! 2. Route 服务模式：通过 Route 服务进行 SVID 路由（推荐）

use std::sync::Arc;

use anyhow::{Context, Result};
use flare_proto::common::{RequestContext, TenantContext, Message, MessageContent, TextContent, MessageType, MessageSource, MessageStatus, ContentType};
use flare_proto::message::message_service_client::MessageServiceClient;
use flare_proto::message::{
    SendMessageRequest, SendMessageResponse,
    EditMessageRequest, EditMessageResponse,
    AddReactionRequest, AddReactionResponse,
    RemoveReactionRequest, RemoveReactionResponse,
    RecallMessageRequest, RecallMessageResponse,
    MarkMessageReadRequest, MarkMessageReadResponse,
};
use prost::Message as ProstMessage;
use prost_types::Timestamp;
use tokio::sync::Mutex;
use tonic::transport::Channel;
use tracing::{error, info, warn};

use flare_server_core::discovery::ServiceClient;
use crate::infrastructure::signaling::route_client::RouteServiceClient;

/// 消息路由服务
pub struct MessageRouter {
    /// 直接路由客户端（message-orchestrator）
    client: Arc<Mutex<Option<MessageServiceClient<Channel>>>>,
    service_name: String,
    default_tenant_id: String,
    service_client: Arc<Mutex<Option<ServiceClient>>>,
    /// Route 服务客户端（用于 SVID 路由）
    route_client: Option<Arc<RouteServiceClient>>,
    /// 是否使用 Route 服务
    use_route_service: bool,
    /// 默认 SVID
    default_svid: String,
}

impl MessageRouter {
    /// 创建新的消息路由服务（使用服务名称，内部创建服务发现）
    pub fn new(
        service_name: String,
        default_tenant_id: String,
        route_client: Option<Arc<RouteServiceClient>>,
        use_route_service: bool,
        default_svid: String,
    ) -> Self {
        Self {
            client: Arc::new(Mutex::new(None)),
            service_name,
            default_tenant_id,
            service_client: Arc::new(Mutex::new(None)),
            route_client,
            use_route_service,
            default_svid,
        }
    }

    /// 使用 ServiceClient 创建新的消息路由服务（推荐，通过 wire 注入）
    pub fn with_service_client(
        service_client: ServiceClient,
        default_tenant_id: String,
        route_client: Option<Arc<RouteServiceClient>>,
        use_route_service: bool,
        default_svid: String,
    ) -> Self {
        Self {
            client: Arc::new(Mutex::new(None)),
            service_name: String::new(), // 不需要 service_name
            default_tenant_id,
            service_client: Arc::new(Mutex::new(Some(service_client))),
            route_client,
            use_route_service,
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
            
            let discover = discover.ok_or_else(|| {
                anyhow::anyhow!("Service discovery not configured")
            })?;
            
            *service_client_guard = Some(ServiceClient::new(discover));
        }
        
        let service_client = service_client_guard.as_mut().ok_or_else(|| {
            anyhow::anyhow!("Service client not initialized")
        })?;
        let channel = service_client.get_channel().await
            .map_err(|e| {
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
    /// 
    /// 根据配置选择路由方式：
    /// - 如果 use_route_service=true，通过 Route 服务路由（带 SVID）
    /// - 否则，直接路由到 message-orchestrator（向后兼容）
    pub async fn route_message(
        &self,
        user_id: &str,
        session_id: &str,
        payload: Vec<u8>,
        tenant_id: Option<&str>,
    ) -> Result<SendMessageResponse> {
        // 如果使用 Route 服务，通过 Route 服务路由
        if self.use_route_service {
            if let Some(ref route_client) = self.route_client {
                return self.route_via_route_service(
                    user_id,
                    session_id,
                    payload,
                    tenant_id,
                    route_client,
                ).await;
            } else {
                warn!(
                    "Route service is enabled but route_client is not available, falling back to direct routing"
                );
            }
        }
        
        // 直接路由模式（向后兼容）
        self.route_direct(user_id, session_id, payload, tenant_id).await
    }

    /// 通过 Route 服务路由消息
    async fn route_via_route_service(
        &self,
        user_id: &str,
        session_id: &str,
        payload: Vec<u8>,
        tenant_id: Option<&str>,
        route_client: &RouteServiceClient,
    ) -> Result<SendMessageResponse> {
        // 构建 SendMessageRequest（与直接路由模式相同）
        let text_content = String::from_utf8_lossy(&payload).to_string();
        let message_content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::Text(
                TextContent {
                    text: text_content,
                    mentions: vec![],
                },
            )),
        };

        let message = Message {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            client_msg_id: String::new(), // 客户端消息ID（可选）
            sender_id: user_id.to_string(),
            source: MessageSource::User as i32, // 消息来源（枚举）
            sender_nickname: String::new(), // 发送者昵称（可选）
            sender_avatar_url: String::new(), // 发送者头像URL（可选）
            sender_platform_id: String::new(), // 发送平台ID（可选）
            receiver_ids: vec![],
            receiver_id: String::new(),
            group_id: String::new(), // 群ID（可选）
            content: Some(message_content),
            content_type: ContentType::PlainText as i32, // 内容类型（枚举）
            timestamp: Some(Timestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: 0,
            }),
            created_at: None, // 创建时间（可选）
            seq: 0, // 消息序列号（可选）
            message_type: MessageType::Text as i32, // 消息类型（枚举）
            business_type: "chatroom".to_string(),
            session_type: "group".to_string(),
            status: MessageStatus::Created as i32, // 消息状态（枚举）
            extra: {
                let mut extra = std::collections::HashMap::new();
                extra.insert("source".to_string(), "access_gateway".to_string());
                extra.insert("chatroom".to_string(), "true".to_string());
                if let Some(tenant_id) = tenant_id {
                    extra.insert("tenant_id".to_string(), tenant_id.to_string());
                }
                extra
            },
            attributes: std::collections::HashMap::new(),
            is_recalled: false,
            recalled_at: None,
            recall_reason: String::new(), // 撤回原因（可选）
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
            audit: None,
            attachments: vec![],
            tags: vec![],
            visibility: std::collections::HashMap::new(),
            read_by: vec![],
            operations: vec![],
            timeline: None,
            forward_info: None, // 转发信息（可选）
            offline_push_info: None, // 离线推送信息（可选）
        };

        let request_context = RequestContext {
            request_id: uuid::Uuid::new_v4().to_string(),
            trace: None,
            actor: Some(flare_proto::common::ActorContext {
                actor_id: user_id.to_string(),
                r#type: flare_proto::common::ActorType::User as i32,
                roles: vec![],
                attributes: std::collections::HashMap::new(),
            }),
            device: None,
            channel: String::new(),
            user_agent: String::new(),
            attributes: std::collections::HashMap::new(),
        };

        let send_request = SendMessageRequest {
            session_id: session_id.to_string(),
            message: Some(message),
            sync: false,
            context: Some(request_context.clone()),
            tenant: Some(TenantContext {
                tenant_id: tenant_id.unwrap_or(&self.default_tenant_id).to_string(),
                business_type: "chatroom".to_string(),
                environment: String::new(),
                organization_id: String::new(),
                labels: std::collections::HashMap::new(),
                attributes: std::collections::HashMap::new(),
            }),
            payload: None,
        };

        // 序列化 SendMessageRequest 为 protobuf
        let mut request_bytes = Vec::new();
        send_request.encode(&mut request_bytes)
            .context("Failed to encode SendMessageRequest")?;

        // 通过 Route 服务路由
        let route_response = route_client.route_message(
            user_id,
            &self.default_svid,
            request_bytes,
            Some(request_context),
            Some(TenantContext {
                tenant_id: tenant_id.unwrap_or(&self.default_tenant_id).to_string(),
                business_type: "chatroom".to_string(),
                environment: String::new(),
                organization_id: String::new(),
                labels: std::collections::HashMap::new(),
                attributes: std::collections::HashMap::new(),
            }),
        ).await
        .context("Failed to route message via Route Service")?;

        // 解析响应
        let response = SendMessageResponse::decode(&route_response.response[..])
            .context("Failed to decode SendMessageResponse from Route Service")?;

        info!(
            user_id = %user_id,
            session_id = %session_id,
            message_id = %response.message_id,
            svid = %self.default_svid,
            "Message routed via Route Service successfully"
        );

        Ok(response)
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
        
        let client = client_guard.as_mut()
            .ok_or_else(|| anyhow::anyhow!("Message Router client not available after initialization"))?;

        // 构建发送请求
        let request = self.build_send_message_request(
            user_id,
            session_id,
            payload,
            tenant_id,
        )?;

        // 发送请求
        let response = match client.send_message(tonic::Request::new(request)).await {
            Ok(resp) => resp,
            Err(e) => {
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
                return Err(anyhow::anyhow!("Failed to send message to Message Orchestrator: {}", e));
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
        let mut client_guard = self.client.lock().await;
        
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
            return Err(anyhow::anyhow!("Failed to initialize Message Router after retries: {}", err));
        }
        
        // 重新获取锁
        Ok(self.client.lock().await)
    }

    /// 调用 EditMessage（按属性更新，兼容当前服务端实现）
    pub async fn route_edit_message(
        &self,
        message_id: &str,
        attributes: std::collections::HashMap<String, String>,
        tenant_id: Option<&str>,
        actor_id: &str,
    ) -> Result<EditMessageResponse> {
        let mut client_guard = self.client.lock().await;
        if client_guard.is_none() { self.initialize().await?; client_guard = self.client.lock().await; }
        let client = client_guard.as_mut().ok_or_else(|| anyhow::anyhow!("Message Router client not available"))?;

        let req = EditMessageRequest {
            message_id: message_id.to_string(),
            operator_id: actor_id.to_string(),
            attributes,
            tags: vec![],
            context: Some(self.build_request_context(actor_id)),
            tenant: Some(self.build_tenant_context(tenant_id)),
        };
        let resp = client.edit_message(tonic::Request::new(req)).await?.into_inner();
        Ok(resp)
    }

    /// 调用 AddReaction
    pub async fn route_add_reaction(
        &self,
        message_id: &str,
        emoji: &str,
        tenant_id: Option<&str>,
        actor_id: &str,
    ) -> Result<AddReactionResponse> {
        let mut client_guard = self.ensure_client().await?;
        let client = client_guard.as_mut()
            .ok_or_else(|| anyhow::anyhow!("Message Router client not available"))?;
        let req = AddReactionRequest {
            message_id: message_id.to_string(),
            user_id: actor_id.to_string(),
            emoji: emoji.to_string(),
            context: Some(self.build_request_context(actor_id)),
            tenant: Some(self.build_tenant_context(tenant_id)),
        };
        let resp = client.add_reaction(tonic::Request::new(req)).await?.into_inner();
        Ok(resp)
    }

    /// 调用 RemoveReaction
    pub async fn route_remove_reaction(
        &self,
        message_id: &str,
        emoji: &str,
        tenant_id: Option<&str>,
        actor_id: &str,
    ) -> Result<RemoveReactionResponse> {
        let mut client_guard = self.ensure_client().await?;
        let client = client_guard.as_mut()
            .ok_or_else(|| anyhow::anyhow!("Message Router client not available"))?;
        let req = RemoveReactionRequest {
            message_id: message_id.to_string(),
            user_id: actor_id.to_string(),
            emoji: emoji.to_string(),
            context: Some(self.build_request_context(actor_id)),
            tenant: Some(self.build_tenant_context(tenant_id)),
        };
        let resp = client.remove_reaction(tonic::Request::new(req)).await?.into_inner();
        Ok(resp)
    }

    /// 调用 RecallMessage
    pub async fn route_recall_message(
        &self,
        message_id: &str,
        tenant_id: Option<&str>,
        actor_id: &str,
    ) -> Result<RecallMessageResponse> {
        let mut client_guard = self.ensure_client().await?;
        let client = client_guard.as_mut()
            .ok_or_else(|| anyhow::anyhow!("Message Router client not available"))?;
        let req = RecallMessageRequest {
            message_id: message_id.to_string(),
            operator_id: actor_id.to_string(),
            reason: String::new(),
            recall_time_limit_seconds: 0,
            context: Some(self.build_request_context(actor_id)),
            tenant: Some(self.build_tenant_context(tenant_id)),
        };
        let resp = client.recall_message(tonic::Request::new(req)).await?.into_inner();
        Ok(resp)
    }

    /// 调用 MarkMessageRead（按会话与游标）
    pub async fn route_mark_read(
        &self,
        message_id: &str,
        tenant_id: Option<&str>,
        actor_id: &str,
    ) -> Result<MarkMessageReadResponse> {
        let mut client_guard = self.ensure_client().await?;
        let client = client_guard.as_mut()
            .ok_or_else(|| anyhow::anyhow!("Message Router client not available"))?;
        let req = MarkMessageReadRequest {
            message_id: message_id.to_string(),
            user_id: actor_id.to_string(),
            context: Some(self.build_request_context(actor_id)),
            tenant: Some(self.build_tenant_context(tenant_id)),
        };
        let resp = client.mark_message_read(tonic::Request::new(req)).await?.into_inner();
        Ok(resp)
    }

    /// 构建 SendMessageRequest（内部辅助函数）
    /// 
    /// 提取消息构建逻辑，减少代码重复
    fn build_send_message_request(
        &self,
        user_id: &str,
        session_id: &str,
        payload: Vec<u8>,
        tenant_id: Option<&str>,
    ) -> Result<SendMessageRequest> {
        // 构建消息内容
        let text_content = String::from_utf8_lossy(&payload).to_string();
        let message_content = MessageContent {
            content: Some(flare_proto::common::message_content::Content::Text(
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
            client_msg_id: String::new(), // 客户端消息ID（可选）
            sender_id: user_id.to_string(),
            source: MessageSource::User as i32, // 消息来源（枚举）
            sender_nickname: String::new(), // 发送者昵称（可选）
            sender_avatar_url: String::new(), // 发送者头像URL（可选）
            sender_platform_id: String::new(), // 发送平台ID（可选）
            receiver_ids: vec![], // 聊天室消息，接收者为空（广播）
            receiver_id: String::new(),
            group_id: String::new(), // 群ID（可选）
            content: Some(message_content),
            content_type: ContentType::PlainText as i32, // 内容类型（枚举）
            timestamp: Some(Timestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: 0,
            }),
            created_at: None, // 创建时间（可选）
            seq: 0, // 消息序列号（可选）
            message_type: MessageType::Text as i32, // MessageType::Text = 1（枚举）
            business_type: "chatroom".to_string(),
            session_type: "group".to_string(), // 聊天室是群组类型
            status: MessageStatus::Created as i32, // 消息状态（枚举）
            extra: {
                let mut extra = std::collections::HashMap::new();
                extra.insert("source".to_string(), "access_gateway".to_string());
                extra.insert("chatroom".to_string(), "true".to_string());
                if let Some(tenant_id) = tenant_id {
                    extra.insert("tenant_id".to_string(), tenant_id.to_string());
                }
                extra
            },
            attributes: std::collections::HashMap::new(),
            is_recalled: false,
            recalled_at: None,
            recall_reason: String::new(), // 撤回原因（可选）
            is_burn_after_read: false,
            burn_after_seconds: 0,
            tenant: Some(self.build_tenant_context(tenant_id)),
            audit: None,
            attachments: vec![],
            tags: vec![],
            visibility: std::collections::HashMap::new(),
            read_by: vec![],
            operations: vec![],
            timeline: None,
            forward_info: None, // 转发信息（可选）
            offline_push_info: None, // 离线推送信息（可选）
        };

        // 构建请求上下文
        let request_context = self.build_request_context(user_id);

        // 构建发送请求
        Ok(SendMessageRequest {
            session_id: session_id.to_string(),
            message: Some(message),
            sync: false, // 异步模式，立即返回
            context: Some(request_context),
            tenant: Some(self.build_tenant_context(tenant_id)),
            payload: None,
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
