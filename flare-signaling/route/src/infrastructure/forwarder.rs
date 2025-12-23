//! 消息转发服务
//!
//! 负责将消息转发到对应的业务系统

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use flare_proto::common::{RequestContext, TenantContext};
use flare_proto::message::message_service_client::MessageServiceClient;
use flare_proto::message::{SendMessageRequest, SendMessageResponse};
use prost::Message as ProstMessage;
use tokio::sync::Mutex;
use tonic::transport::Channel;
use tracing::{debug, info};

use crate::domain::repository::RouteRepository;
use flare_server_core::discovery::ServiceClient;

/// SVID 常量定义
pub mod svid {
    pub const IM: &str = "svid.im";
    pub const CUSTOMER_SERVICE: &str = "svid.customer";
    pub const AI_BOT: &str = "svid.ai.bot";

    /// 从旧的 SVID 格式转换为新格式
    pub fn normalize(svid: &str) -> String {
        match svid.to_uppercase().as_str() {
            "IM" => IM.to_string(),
            "CUSTOMER_SERVICE" | "CS" => CUSTOMER_SERVICE.to_string(),
            "AI_BOT" | "AI" => AI_BOT.to_string(),
            _ => {
                // 如果已经是新格式（包含点），直接返回
                if svid.contains('.') {
                    svid.to_string()
                } else {
                    // 默认转换为 svid.{svid}
                    format!("svid.{}", svid.to_lowercase())
                }
            }
        }
    }
}

/// 消息转发服务
pub struct MessageForwarder {
    /// IM 业务系统客户端（message-orchestrator）
    im_client: Arc<Mutex<Option<MessageServiceClient<Channel>>>>,
    /// 其他业务系统客户端缓存
    business_clients: Arc<Mutex<HashMap<String, MessageServiceClient<Channel>>>>,
    /// 服务发现客户端
    service_client: Arc<Mutex<Option<ServiceClient>>>,
    /// Router（已废弃，建议使用 MessageRoutingDomainService）
    #[deprecated(note = "Use MessageRoutingDomainService instead")]
    router: Arc<Mutex<Option<Arc<crate::service::router::Router>>>>,
    /// 默认租户ID
    default_tenant_id: String,
}

impl MessageForwarder {
    /// 创建新的消息转发服务
    pub fn new(default_tenant_id: String) -> Self {
        Self {
            im_client: Arc::new(Mutex::new(None)),
            business_clients: Arc::new(Mutex::new(HashMap::new())),
            service_client: Arc::new(Mutex::new(None)),
            router: Arc::new(Mutex::new(None)),
            default_tenant_id,
        }
    }

    /// 使用 ServiceClient 创建（推荐，通过 wire 注入）
    pub fn with_service_client(service_client: ServiceClient, default_tenant_id: String) -> Self {
        Self {
            im_client: Arc::new(Mutex::new(None)),
            business_clients: Arc::new(Mutex::new(HashMap::new())),
            service_client: Arc::new(Mutex::new(Some(service_client))),
            router: Arc::new(Mutex::new(None)),
            default_tenant_id,
        }
    }

    /// 注入 Router（已废弃，建议使用 MessageRoutingDomainService）
    #[deprecated(note = "Use MessageRoutingDomainService instead")]
    pub async fn set_router(&self, router: Arc<crate::service::router::Router>) {
        let mut guard = self.router.lock().await;
        *guard = Some(router);
    }

    /// 初始化 IM 客户端（message-orchestrator）
    async fn ensure_im_client(&self) -> Result<()> {
        let mut client_guard = self.im_client.lock().await;
        if client_guard.is_some() {
            return Ok(());
        }

        info!("Initializing IM service client (message-orchestrator)...");

            // 创建服务发现器（使用常量，根据 SVID 过滤）
            // 注意：这里使用默认的 SVID (svid.im)，如果需要支持多个 SVID，需要修改为根据请求的 SVID 动态创建
            use flare_im_core::service_names::{MESSAGE_ORCHESTRATOR, get_service_name};
            use flare_im_core::config::app_config;
            use flare_im_core::discovery::create_discover_from_registry_config_with_filters;
            
            let message_orchestrator_service = get_service_name(MESSAGE_ORCHESTRATOR);
        
        let mut service_client_guard = self.service_client.lock().await;
        if service_client_guard.is_none() {
            let app_config = app_config();
            
            // 根据 SVID 过滤服务实例（默认使用 svid.im）
            let mut tag_filters = std::collections::HashMap::new();
            tag_filters.insert("svid".to_string(), svid::IM.to_string());
            
            info!(
                service = %message_orchestrator_service,
                svid_filter = %svid::IM,
                tag_filters = ?tag_filters,
                "Creating service discover with SVID filter"
            );
            
            let discover = if let Some(registry_config) = &app_config.core.registry {
                create_discover_from_registry_config_with_filters(
                    registry_config,
                    &message_orchestrator_service,
                    Some(tag_filters),
                )
                .await
                .map_err(|e| {
                    anyhow::anyhow!(
                        "Failed to create service discover for {} with SVID filter (svid={}): {}",
                        message_orchestrator_service,
                        svid::IM,
                        e
                    )
                })?
            } else {
                // 如果没有配置 registry，返回 None（后续会报错）
                return Err(anyhow::anyhow!(
                    "Service discovery not configured for {}",
                    message_orchestrator_service
                ));
            };
            
            info!(
                service = %message_orchestrator_service,
                svid_filter = %svid::IM,
                "Service discover created successfully"
            );

            *service_client_guard = Some(ServiceClient::new(discover));
        }

        let service_client = service_client_guard.as_mut().unwrap();
        // 添加超时保护，避免服务发现阻塞过长时间
        info!("Attempting to get channel from service discovery (timeout: 3s)...");
        let channel = tokio::time::timeout(
            std::time::Duration::from_secs(3), // 3秒超时
            service_client.get_channel(),
        )
            .await
        .map_err(|_| {
            anyhow::anyhow!("Timeout waiting for service discovery to get channel for {} (3s) - no service instances found matching svid={}", message_orchestrator_service, svid::IM)
        })?
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to get channel from service discovery for {} (svid={}): {}",
                    message_orchestrator_service,
                    svid::IM,
                    e
                )
            })?;
        
        info!(
            service = %message_orchestrator_service,
            svid_filter = %svid::IM,
            "Channel obtained from service discovery"
        );

        let client = MessageServiceClient::new(channel);
        *client_guard = Some(client);

        info!("✅ IM service client initialized successfully");
        Ok(())
    }

    /// 根据 endpoint 获取或创建业务系统客户端
    ///
    /// endpoint 可以是：
    /// - 服务名（通过服务发现解析）
    /// - gRPC URL（http://host:port 或 https://host:port）
    /// - host:port 格式
    async fn get_business_client(&self, endpoint: &str) -> Result<MessageServiceClient<Channel>> {
        // 判断 endpoint 是服务名还是 URL
        let channel = if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
            // 直接 URL
            tonic::transport::Endpoint::from_shared(endpoint.to_string())
                .context("Invalid endpoint format")?
                .connect()
                .await
                .context("Failed to connect to business service")?
        } else if endpoint.contains(':') && !endpoint.contains('.') {
            // host:port 格式
            let endpoint_url = format!("http://{}", endpoint);
            tonic::transport::Endpoint::from_shared(endpoint_url)
                .context("Invalid endpoint format")?
                .connect()
                .await
                .context("Failed to connect to business service")?
        } else {
            // 服务名（通过服务发现）
            let discover_result = flare_im_core::discovery::create_discover(endpoint)
                .await
                .map_err(|e| {
                    anyhow::anyhow!("Failed to create service discover for {}: {}", endpoint, e)
                })?;

            let discover = discover_result.ok_or_else(|| {
                anyhow::anyhow!("Service discovery not configured for {}", endpoint)
            })?;

            let mut service_client = ServiceClient::new(discover);
            // 添加超时保护，避免服务发现阻塞过长时间
            tokio::time::timeout(
                std::time::Duration::from_secs(3), // 3秒超时
                service_client.get_channel(),
            )
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "Timeout waiting for service discovery to get channel for {} (3s)",
                    endpoint
                )
            })?
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to get channel from service discovery for {}: {}",
                    endpoint,
                    e
                )
            })?
        };

        Ok(MessageServiceClient::new(channel))
    }

    /// 转发消息到业务系统
    ///
    /// 返回 (端点, 响应数据) 元组
    pub async fn forward_message(
        &self,
        svid: &str,
        payload: Vec<u8>,
        context: Option<RequestContext>,
        tenant: Option<TenantContext>,
        route_repository: Option<Arc<dyn RouteRepository + Send + Sync>>,
    ) -> Result<(String, Vec<u8>)> {
        let normalized_svid = svid::normalize(svid);
        debug!(svid = %normalized_svid, "Forwarding message to business system");

        // 构造路由上下文（从 RequestContext.attributes 提取）
        let route_ctx = crate::domain::service::RouteContext {
            svid: normalized_svid.clone(),
            conversation_id: context
                .as_ref()
                .and_then(|c| c.attributes.get("conversation_id").cloned()),
            user_id: context
                .as_ref()
                .and_then(|c| c.actor.as_ref().map(|a| a.actor_id.clone())),
            tenant_id: tenant.as_ref().map(|t| t.tenant_id.clone()),
            client_geo: context
                .as_ref()
                .and_then(|c| c.attributes.get("geo").cloned()),
            login_gateway: context
                .as_ref()
                .and_then(|c| c.attributes.get("login_gateway").cloned()),
        };

        // 根据 SVID 选择转发目标
        match normalized_svid.as_str() {
            svid::IM => {
                // 转发到 message-orchestrator
                self.forward_to_im(payload, context, tenant).await
            }
            _ => {
                // 其他业务系统：使用路由仓储解析端点并转发
                if let Some(repo) = route_repository {
                    // 从路由仓储查找端点
                    use crate::domain::model::Svid;
                    let svid = Svid::new(normalized_svid.clone())
                        .map_err(|e| anyhow::anyhow!("Invalid SVID: {}", e))?;
                    
                    let endpoint = match repo.find_by_svid(svid.as_str()).await {
                        Ok(Some(route)) => route.endpoint().as_str().to_string(),
                        Ok(None) => {
                            return Err(anyhow::anyhow!(
                                "Business service not found for SVID {}",
                                normalized_svid
                            ));
                        }
                        Err(e) => {
                            return Err(anyhow::anyhow!(
                                "Failed to resolve route for SVID {}: {}",
                                normalized_svid,
                                e
                            ));
                        }
                    };

                    // 连接业务系统客户端并转发消息
                    let mut client = self.get_business_client(&endpoint).await?;

                    // 构造转发请求
                    let request = flare_proto::message::SendMessageRequest {
                        conversation_id: "".to_string(), // 需要根据实际逻辑填充
                        message: None,              // 消息内容在 payload 中
                        sync: false,
                        context,
                        tenant,
                    };

                    // 发送请求到业务系统
                    let response = client
                        .send_message(tonic::Request::new(request))
                        .await
                        .context("Failed to send message to business service")?;

                    let response_inner = response.into_inner();

                    // 序列化响应
                    let mut response_bytes = Vec::new();
                    flare_proto::message::SendMessageResponse::encode(
                        &response_inner,
                        &mut response_bytes,
                    )
                    .context("Failed to encode SendMessageResponse")?;

                    info!(
                        "✅ Message forwarded to business service successfully: SVID={}, Endpoint={}",
                        normalized_svid, endpoint
                    );
                    Ok((endpoint, response_bytes))
                } else {
                    Err(anyhow::anyhow!(
                        "Route repository not available, cannot resolve endpoint for SVID {}",
                        normalized_svid
                    ))
                }
            }
        }
    }

    /// 转发消息到 IM 业务系统（message-orchestrator）
    async fn forward_to_im(
        &self,
        payload: Vec<u8>,
        context: Option<RequestContext>,
        tenant: Option<TenantContext>,
    ) -> Result<(String, Vec<u8>)> {
        // 确保客户端已初始化
        self.ensure_im_client()
            .await
            .context("Failed to initialize IM service client")?;

        // 解析 payload 为 Message 对象（Gateway 发送的是 Message 的 protobuf 编码）
        use flare_proto::common::Message;
        
        if payload.is_empty() {
            return Err(anyhow::anyhow!("Empty payload for IM message forwarding"));
        }

        // 解析为 Message 对象
        let message = Message::decode(&payload[..])
            .context("Failed to decode payload as Message")?;

        // 从 context 的 attributes 中获取 conversation_id
        let conversation_id = context
            .as_ref()
            .and_then(|c| c.attributes.get("conversation_id").cloned())
            .unwrap_or_else(|| String::new());

        // 构建 SendMessageRequest
        let request = SendMessageRequest {
            conversation_id,
            message: Some(message),
            sync: false, // 默认异步模式
            context,
            tenant,
        };

        // 发送请求
        let mut client_guard = self.im_client.lock().await;
        let client = client_guard
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("IM service client not available"))?;

        let response = client
            .send_message(tonic::Request::new(request))
            .await
            .context("Failed to send message to message-orchestrator")?;

        let response_inner = response.into_inner();

        // 序列化响应
        let mut response_bytes = Vec::new();
        SendMessageResponse::encode(&response_inner, &mut response_bytes)
            .context("Failed to encode SendMessageResponse")?;

        // 获取服务名作为端点标识
        use flare_im_core::service_names::{MESSAGE_ORCHESTRATOR, get_service_name};
        let endpoint = get_service_name(MESSAGE_ORCHESTRATOR);

        info!("✅ Message forwarded to IM service successfully, endpoint: {}", endpoint);
        Ok((endpoint, response_bytes))
    }
}
