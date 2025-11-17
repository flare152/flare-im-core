//! Gateway Router（跨地区路由组件）
//!
//! 根据gateway_id路由到对应的Access Gateway，支持单地区/多地区自适应部署。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use async_trait::async_trait;
use flare_proto::access_gateway::{
    access_gateway_client::AccessGatewayClient, PushMessageRequest, PushMessageResponse,
};
use tokio::sync::RwLock;
use tonic::transport::{Channel, Endpoint};
use tracing::{debug, error, info, warn};

use crate::config::PushServerConfig;

/// Gateway Router trait
#[async_trait]
pub trait GatewayRouterTrait: Send + Sync {
    /// 路由推送请求到Access Gateway
    async fn route_push_message(
        &self,
        gateway_id: &str,
        request: PushMessageRequest,
    ) -> Result<PushMessageResponse>;
}

/// Gateway Router实现
pub struct GatewayRouterImpl {
    config: Arc<PushServerConfig>,
    /// 连接池（gateway_id -> client）
    connection_pool: Arc<RwLock<HashMap<String, AccessGatewayClient<Channel>>>>,
}

impl GatewayRouterImpl {
    /// 创建Gateway Router
    pub fn new(config: Arc<PushServerConfig>) -> Arc<Self> {
        Arc::new(Self {
            config,
            connection_pool: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// 判断是否为本地网关
    fn is_local_gateway(&self, gateway_id: &str) -> bool {
        self.config
            .local_gateway_id
            .as_ref()
            .map(|local_id| local_id == gateway_id)
            .unwrap_or(false)
            || self.config.gateway_deployment_mode == "single_region"
    }

    /// 获取或创建Access Gateway客户端
    async fn get_or_create_client(
        &self,
        gateway_id: &str,
    ) -> Result<AccessGatewayClient<Channel>> {
        // 先检查连接池
        {
            let pool = self.connection_pool.read().await;
            if let Some(client) = pool.get(gateway_id) {
                return Ok(client.clone());
            }
        }

        // 获取端点
        let endpoint_str = self
            .get_gateway_endpoint(gateway_id)
            .ok_or_else(|| anyhow::anyhow!("Gateway endpoint not found: {}", gateway_id))?;

        // 创建新连接
        let endpoint = Endpoint::from_shared(endpoint_str.clone())
            .context("Invalid gateway endpoint")?
            .timeout(Duration::from_millis(self.config.gateway_router_connection_timeout_ms))
            .connect_timeout(Duration::from_millis(
                self.config.gateway_router_connection_timeout_ms,
            ));

        let channel = endpoint.connect().await.context("Failed to connect to gateway")?;
        let client = AccessGatewayClient::new(channel);

        // 存入连接池
        {
            let mut pool = self.connection_pool.write().await;
            pool.insert(gateway_id.to_string(), client.clone());
        }

        info!(
            gateway_id = %gateway_id,
            endpoint = %endpoint_str,
            "Created new gateway connection"
        );

        Ok(client)
    }

    /// 获取网关端点
    fn get_gateway_endpoint(&self, gateway_id: &str) -> Option<String> {
        // 解析gateway_endpoints配置
        // 格式: "gateway-1:http://localhost:50051,gateway-2:http://localhost:50052"
        for entry in self.config.gateway_endpoints.split(',') {
            let parts: Vec<&str> = entry.split(':').collect();
            if parts.len() >= 2 {
                let config_gateway_id = parts[0].trim();
                let endpoint = parts[1..].join(":").trim().to_string();
                
                if config_gateway_id == gateway_id {
                    return Some(endpoint);
                }
            }
        }
        
        // 如果没有找到，且是单地区部署，使用默认本地端点
        if self.config.gateway_deployment_mode == "single_region" {
            // 尝试从环境变量获取默认端点
            // Access Gateway gRPC 端口 = WebSocket端口 + 2，默认是 60053
            return std::env::var("ACCESS_GATEWAY_ENDPOINT")
                .ok()
                .or_else(|| Some("http://localhost:60053".to_string()));
        }
        
        None
    }
}

#[async_trait]
impl GatewayRouterTrait for GatewayRouterImpl {
    async fn route_push_message(
        &self,
        gateway_id: &str,
        request: PushMessageRequest,
    ) -> Result<PushMessageResponse> {
        info!(
            gateway_id = %gateway_id,
            user_count = request.target_user_ids.len(),
            user_ids = ?request.target_user_ids,
            "Routing push message to gateway"
        );

        // 判断是否为本地网关
        let is_local = self.is_local_gateway(gateway_id);

        if is_local {
            info!(
                gateway_id = %gateway_id,
                "Local gateway, direct call"
            );
        } else {
            info!(
                gateway_id = %gateway_id,
                "Remote gateway, cross-region call"
            );
        }

        // 获取或创建客户端
        info!(
            gateway_id = %gateway_id,
            "Getting or creating gateway client"
        );
        let mut client = match self.get_or_create_client(gateway_id).await {
            Ok(c) => {
                info!(
                    gateway_id = %gateway_id,
                    "Successfully got gateway client"
                );
                c
            }
            Err(e) => {
                warn!(
                    error = %e,
                    gateway_id = %gateway_id,
                    "Failed to get gateway client"
                );
                return Err(e);
            }
        };

        // 调用Access Gateway推送接口
        info!(
            gateway_id = %gateway_id,
            user_count = request.target_user_ids.len(),
            "Calling Access Gateway push_message"
        );
        let response = match client
            .push_message(tonic::Request::new(request))
            .await
        {
            Ok(resp) => {
                info!(
                    gateway_id = %gateway_id,
                    "Successfully pushed message to Access Gateway"
                );
                resp.into_inner()
            }
            Err(e) => {
                warn!(
                    error = %e,
                    gateway_id = %gateway_id,
                    "Failed to call Access Gateway push_message"
                );
                return Err(anyhow::anyhow!("Failed to call access gateway: {}", e).into());
            }
        };

        Ok(response)
    }
}

