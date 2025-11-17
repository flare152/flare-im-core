//! # Gateway Router
//!
//! 跨地区网关路由组件，支持单地区/多地区自适应部署。

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
use tracing::{debug, info, warn};

use crate::handler::access_gateway::GatewayRouter as GatewayRouterTrait;

/// Gateway Router配置
#[derive(Debug, Clone)]
pub struct GatewayRouterConfig {
    /// 本地gateway_id（用于判断是否需要跨地区路由）
    pub local_gateway_id: Option<String>,
    /// 部署模式（single_region / multi_region）
    pub deployment_mode: DeploymentMode,
    /// Access Gateway端点映射（gateway_id -> endpoint）
    pub gateway_endpoints: HashMap<String, String>,
    /// 连接池大小
    pub connection_pool_size: usize,
    /// 连接超时（毫秒）
    pub connection_timeout_ms: u64,
}

/// 部署模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeploymentMode {
    /// 单地区部署
    SingleRegion,
    /// 多地区部署
    MultiRegion,
}

impl Default for DeploymentMode {
    fn default() -> Self {
        Self::SingleRegion
    }
}

impl DeploymentMode {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "multi_region" | "multi-region" | "multi" => Self::MultiRegion,
            _ => Self::SingleRegion,
        }
    }
}

/// Gateway Router实现
pub struct GatewayRouterImpl {
    config: GatewayRouterConfig,
    /// 连接池（gateway_id -> client）
    connection_pool: Arc<RwLock<HashMap<String, AccessGatewayClient<Channel>>>>,
}

impl GatewayRouterImpl {
    /// 创建Gateway Router
    pub fn new(config: GatewayRouterConfig) -> Arc<Self> {
        Arc::new(Self {
            config,
            connection_pool: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// 从环境变量创建Gateway Router
    pub fn from_env() -> Result<Arc<Self>> {
        let deployment_mode = std::env::var("GATEWAY_DEPLOYMENT_MODE")
            .ok()
            .map(|s| DeploymentMode::from_str(&s))
            .unwrap_or_default();

        let local_gateway_id = std::env::var("LOCAL_GATEWAY_ID").ok();

        // 从环境变量加载gateway端点映射
        // 格式: GATEWAY_ENDPOINTS=gateway-1:http://localhost:50051,gateway-2:http://localhost:50052
        let mut gateway_endpoints = HashMap::new();
        if let Ok(endpoints_str) = std::env::var("GATEWAY_ENDPOINTS") {
            for entry in endpoints_str.split(',') {
                let parts: Vec<&str> = entry.split(':').collect();
                if parts.len() >= 2 {
                    let gateway_id = parts[0].trim().to_string();
                    let endpoint = parts[1..].join(":").trim().to_string();
                    gateway_endpoints.insert(gateway_id, endpoint);
                }
            }
        }

        // 如果没有配置，添加默认本地端点
        if gateway_endpoints.is_empty() {
            let default_endpoint = std::env::var("ACCESS_GATEWAY_ENDPOINT")
                .unwrap_or_else(|_| "http://localhost:50051".to_string());
            if let Some(ref gateway_id) = local_gateway_id {
                gateway_endpoints.insert(gateway_id.clone(), default_endpoint);
            } else {
                gateway_endpoints.insert("local".to_string(), default_endpoint);
            }
        }

        let config = GatewayRouterConfig {
            local_gateway_id,
            deployment_mode,
            gateway_endpoints,
            connection_pool_size: std::env::var("GATEWAY_ROUTER_POOL_SIZE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10),
            connection_timeout_ms: std::env::var("GATEWAY_ROUTER_TIMEOUT_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5000),
        };

        Ok(Self::new(config))
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
            .config
            .gateway_endpoints
            .get(gateway_id)
            .ok_or_else(|| anyhow::anyhow!("Gateway endpoint not found: {}", gateway_id))?
            .clone();

        // 创建新连接
        let endpoint = Endpoint::from_shared(endpoint_str.clone())
            .context("Invalid gateway endpoint")?
            .timeout(Duration::from_millis(self.config.connection_timeout_ms))
            .connect_timeout(Duration::from_millis(self.config.connection_timeout_ms));

        let channel = endpoint
            .connect()
            .await
            .context("Failed to connect to gateway")?;

        let client = AccessGatewayClient::new(channel.clone());

        // 存入连接池
        {
            let mut pool = self.connection_pool.write().await;
            pool.insert(gateway_id.to_string(), client.clone());
        }

        info!(
            gateway_id = %gateway_id,
            endpoint = %endpoint_str,
            "Gateway client created and cached"
        );

        Ok(client)
    }

    /// 判断是否为本地网关
    fn is_local_gateway(&self, gateway_id: &str) -> bool {
        match &self.config.local_gateway_id {
            Some(local_id) => gateway_id == local_id,
            None => {
                // 如果没有配置本地gateway_id，单地区部署时认为所有都是本地
                self.config.deployment_mode == DeploymentMode::SingleRegion
            }
        }
    }
}

#[async_trait]
impl GatewayRouterTrait for GatewayRouterImpl {
    async fn route_push_message(
        &self,
        gateway_id: &str,
        request: PushMessageRequest,
    ) -> Result<PushMessageResponse> {
        debug!(
            gateway_id = %gateway_id,
            user_count = request.target_user_ids.len(),
            deployment_mode = ?self.config.deployment_mode,
            "Routing push message to gateway"
        );

        // 判断是否为本地网关
        let is_local = self.is_local_gateway(gateway_id);

        if is_local {
            debug!(
                gateway_id = %gateway_id,
                "Local gateway, direct call"
            );
        } else {
            debug!(
                gateway_id = %gateway_id,
                "Remote gateway, cross-region call"
            );
        }

        // 获取或创建客户端
        let mut client = self.get_or_create_client(gateway_id).await?;

        // 调用Access Gateway推送接口
        let response = client
            .push_message(tonic::Request::new(request))
            .await
            .context("Failed to call access gateway")?
            .into_inner();

        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deployment_mode_from_str() {
        assert_eq!(
            DeploymentMode::from_str("single_region"),
            DeploymentMode::SingleRegion
        );
        assert_eq!(
            DeploymentMode::from_str("multi_region"),
            DeploymentMode::MultiRegion
        );
        assert_eq!(
            DeploymentMode::from_str("multi"),
            DeploymentMode::MultiRegion
        );
    }

    #[test]
    fn test_is_local_gateway() {
        let config = GatewayRouterConfig {
            local_gateway_id: Some("gateway-beijing".to_string()),
            deployment_mode: DeploymentMode::MultiRegion,
            gateway_endpoints: HashMap::new(),
            connection_pool_size: 10,
            connection_timeout_ms: 5000,
        };

        let router = GatewayRouterImpl::new(config);

        // 使用tokio::runtime::Runtime来测试异步函数
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            assert!(router.is_local_gateway("gateway-beijing"));
            assert!(!router.is_local_gateway("gateway-shanghai"));
        });
    }
}

