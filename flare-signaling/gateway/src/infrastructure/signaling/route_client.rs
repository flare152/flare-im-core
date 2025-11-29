//! Route 服务客户端
//!
//! 负责与 Route 服务通信，转发消息到业务系统

use std::sync::Arc;

use anyhow::{Context, Result};
use flare_proto::signaling::signaling_service_client::SignalingServiceClient;
use flare_proto::signaling::{RouteMessageRequest, RouteMessageResponse};
use flare_proto::common::{RequestContext, TenantContext};
use prost::Message as ProstMessage;
use tokio::sync::Mutex;
use tonic::transport::Channel;
use tracing::{error, info, warn};

use flare_server_core::discovery::ServiceClient;

/// Route 服务客户端
pub struct RouteServiceClient {
    client: Arc<Mutex<Option<SignalingServiceClient<Channel>>>>,
    service_name: String,
    default_tenant_id: String,
    service_client: Arc<Mutex<Option<ServiceClient>>>,
}

impl RouteServiceClient {
    /// 创建新的 Route 服务客户端（使用服务名称，内部创建服务发现）
    pub fn new(service_name: String, default_tenant_id: String) -> Self {
        Self {
            client: Arc::new(Mutex::new(None)),
            service_name,
            default_tenant_id,
            service_client: Arc::new(Mutex::new(None)),
        }
    }

    /// 使用 ServiceClient 创建新的 Route 服务客户端（推荐，通过 wire 注入）
    pub fn with_service_client(service_client: ServiceClient, default_tenant_id: String) -> Self {
        Self {
            client: Arc::new(Mutex::new(None)),
            service_name: String::new(),
            default_tenant_id,
            service_client: Arc::new(Mutex::new(Some(service_client))),
        }
    }

    /// 初始化客户端连接
    pub async fn initialize(&self) -> Result<()> {
        info!(
            service_name = %self.service_name,
            "Initializing Route Service client..."
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
                        "Failed to create service discover for route service"
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

        let client = SignalingServiceClient::new(channel);
        *self.client.lock().await = Some(client);
        
        info!(
            service_name = %self.service_name,
            "✅ Route Service client initialized successfully"
        );
        Ok(())
    }

    /// 转发消息到业务系统（通过 Route 服务）
    pub async fn route_message(
        &self,
        user_id: &str,
        svid: &str,
        payload: Vec<u8>,
        context: Option<RequestContext>,
        tenant: Option<TenantContext>,
    ) -> Result<RouteMessageResponse> {
        let mut client_guard = self.client.lock().await;
        
        // 如果客户端未初始化，尝试初始化（最多重试3次）
        if client_guard.is_none() {
            warn!(
                service_name = %self.service_name,
                "Route Service client not initialized, attempting to initialize..."
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
                            "✅ Route Service client initialized successfully after retry"
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
                                "Failed to initialize Route Service client, retrying..."
                            );
                            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                        } else {
                            error!(
                                service_name = %self.service_name,
                                error = %error_msg,
                                "❌ Failed to initialize Route Service client after all retries"
                            );
                        }
                    }
                }
            }
            
            if let Some(ref err) = last_error {
                error!(
                    service_name = %self.service_name,
                    error = %err,
                    "Cannot route message: Route Service client initialization failed"
                );
                return Err(anyhow::anyhow!("Failed to initialize Route Service client after retries: {}", err));
            }
            
            client_guard = self.client.lock().await;
        }

        let client = client_guard.as_mut()
            .ok_or_else(|| anyhow::anyhow!("Route Service client not available after initialization"))?;

        // 构建 RouteMessageRequest
        let request = RouteMessageRequest {
            user_id: user_id.to_string(),
            svid: svid.to_string(),
            payload,
            context,
            tenant,
        };

        // 发送请求
        let response = match client.route_message(tonic::Request::new(request)).await {
            Ok(resp) => resp.into_inner(),
            Err(e) => {
                error!(
                    error = %e,
                    user_id = %user_id,
                    svid = %svid,
                    "Failed to route message via Route Service"
                );
                // 如果连接失败，清除客户端以便下次重试
                {
                    let mut client_guard = self.client.lock().await;
                    *client_guard = None;
                }
                return Err(anyhow::anyhow!("Failed to route message via Route Service: {}", e));
            }
        };

        if !response.success {
            return Err(anyhow::anyhow!(
                "Route Service returned error: {}",
                response.error_message
            ));
        }

        info!(
            user_id = %user_id,
            svid = %svid,
            "Message routed via Route Service successfully"
        );

        Ok(response)
    }

    /// 检查客户端是否已连接
    pub async fn is_connected(&self) -> bool {
        self.client.lock().await.is_some()
    }
}

