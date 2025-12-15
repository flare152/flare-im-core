pub mod connection_quality_service;
pub mod multi_device_push_service;
pub mod push_domain_service;
pub mod session_domain_service;
pub mod subscription_service;
pub mod connection_domain_service;

// 添加Online服务客户端的导入
mod online_client;
pub use online_client::OnlineServiceClient;

pub use connection_quality_service::{ConnectionQualityMetrics, ConnectionQualityService, QualityLevel};
pub use multi_device_push_service::MultiDevicePushService;
pub use push_domain_service::{DomainPushResult, PushDomainService};
pub use session_domain_service::SessionDomainService;
pub use subscription_service::SubscriptionService;
pub use connection_domain_service::{ConnectionDomainService, ConnectionDomainServiceConfig};

#[cfg(test)]
mod push_domain_service_test;

use std::sync::Arc;

use crate::domain::repository::{ConnectionQuery, SignalingGateway};
use crate::interface::connection::LongConnectionHandler;

/// 网关领域服务配置
#[derive(Debug, Clone)]
pub struct GatewayServiceConfig {
    /// 网关ID
    pub gateway_id: String,
    /// Online服务端点
    pub online_service_endpoint: Option<String>,
}

impl Default for GatewayServiceConfig {
    fn default() -> Self {
        Self {
            gateway_id: "gateway-1".to_string(),
            online_service_endpoint: Some("http://127.0.0.1:50061".to_string()), // 默认连接本地Online服务
        }
    }
}

/// 网关领域服务
pub struct GatewayService {
    pub connection_service: Arc<ConnectionDomainService>,
    pub session_service: Arc<SessionDomainService>,
    pub push_service: Arc<PushDomainService>,
    pub quality_service: Arc<ConnectionQualityService>,
    pub multi_device_push_service: Arc<MultiDevicePushService>,
    pub subscription_service: Arc<SubscriptionService>,
    /// Online服务客户端
    pub online_service_client: Option<Arc<OnlineServiceClient>>,
}

impl GatewayService {
    pub async fn new(
        signaling_gateway: Arc<dyn SignalingGateway>,
        connection_query: Arc<dyn ConnectionQuery>,
        connection_handler: Arc<LongConnectionHandler>,
        config: GatewayServiceConfig,
    ) -> Self {
        let connection_service = Arc::new(ConnectionDomainService::new(
            signaling_gateway.clone(),
            Arc::new(ConnectionQualityService::new()),
            ConnectionDomainServiceConfig {
                gateway_id: config.gateway_id.clone(),
            },
        ));
        
        let session_service = Arc::new(SessionDomainService::new(
            signaling_gateway.clone(),
            Arc::new(ConnectionQualityService::new()),
            config.gateway_id.clone(), // 从配置中获取
        ));
        
        let push_service = Arc::new(PushDomainService::new(
            connection_handler.clone(),
            connection_query.clone(),
        ));
        
        let quality_service = Arc::new(ConnectionQualityService::new());
        
        // 初始化Online服务客户端
        let online_service_client = if let Some(endpoint) = &config.online_service_endpoint {
            match OnlineServiceClient::new(endpoint.clone()).await {
                Ok(client) => {
                    tracing::info!(endpoint = %endpoint, "Successfully connected to Online service");
                    Some(Arc::new(client))
                }
                Err(e) => {
                    tracing::warn!(error = ?e, endpoint = %endpoint, "Failed to connect to Online service");
                    None
                }
            }
        } else {
            None
        };
        
        // 创建MultiDevicePushService时传入Online服务客户端
        let multi_device_push_service = Arc::new(MultiDevicePushService::new(
            quality_service.clone(),
            online_service_client.as_ref().map(|client| client.get_user_service_client()),
        ));
        
        let subscription_service = Arc::new(SubscriptionService::new());
        
        Self {
            connection_service,
            session_service,
            push_service,
            quality_service,
            multi_device_push_service,
            subscription_service,
            online_service_client,
        }
    }
}