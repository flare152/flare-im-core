//! # 服务路由层
//!
//! 提供gRPC服务路由功能，根据服务类型自动路由到对应的Handler。

use std::sync::Arc;

use anyhow::Result;
use tonic::Request;

use crate::handler::admin::{
    ConfigServiceHandler, HookServiceHandler, MetricsServiceHandler, TenantServiceHandler,
};
use crate::handler::business::{
    MessageServiceHandler, PushServiceHandler, SessionServiceHandler, UserServiceHandler,
};
use crate::handler::gateway::GatewayHandler;
use crate::infrastructure::{GrpcPushClient, GrpcSignalingClient, GrpcStorageClient};
use crate::repository::hook::HookConfigRepositoryImpl;
use crate::repository::tenant::TenantRepositoryImpl;

/// 服务路由器
pub struct ServiceRouter {
    /// 业务端Handler
    pub message_handler: Arc<MessageServiceHandler>,
    pub session_handler: Arc<SessionServiceHandler>,
    pub user_handler: Arc<UserServiceHandler>,
    pub push_handler: Arc<PushServiceHandler>,
    
    /// 管理端Handler
    pub tenant_handler: Arc<TenantServiceHandler>,
    pub hook_handler: Arc<HookServiceHandler>,
    pub metrics_handler: Arc<MetricsServiceHandler>,
    pub config_handler: Arc<ConfigServiceHandler>,
    
    /// 核心通信Handler
    pub communication_handler: Arc<GatewayHandler>,
}

impl ServiceRouter {
    /// 创建服务路由器
    pub async fn new(
        signaling_client: Arc<GrpcSignalingClient>,
        storage_client: Arc<GrpcStorageClient>,
        push_client: Arc<GrpcPushClient>,
        tenant_repository: Option<Arc<TenantRepositoryImpl>>,
        hook_config_repository: Option<Arc<HookConfigRepositoryImpl>>,
    ) -> Result<Self> {
        // 创建业务端Handler
        let message_handler = Arc::new(MessageServiceHandler::new(storage_client.clone()));
        let session_handler = Arc::new(SessionServiceHandler::new());
        let user_handler = Arc::new(UserServiceHandler::new(signaling_client.clone()));
        let push_handler = Arc::new(PushServiceHandler::new(push_client.clone()));
        
        // 创建管理端Handler（如果提供了仓储）
        let tenant_handler = if let Some(repo) = tenant_repository {
            Arc::new(TenantServiceHandler::new(repo))
        } else {
            // 如果没有提供租户仓储，创建一个占位Handler（功能受限）
            tracing::warn!("TenantRepository not provided, tenant handler will have limited functionality");
            // 这里需要创建一个不依赖仓储的Handler，暂时先panic提示需要配置
            return Err(anyhow::anyhow!("TenantRepository is required for ServiceRouter"));
        };
        
        // HookServiceHandler需要hook_engine_endpoint，而不是repository
        // 这里暂时使用默认endpoint，实际应该从配置中获取
        let hook_handler = Arc::new(HookServiceHandler::new(
            std::env::var("HOOK_ENGINE_ENDPOINT")
                .unwrap_or_else(|_| "http://localhost:50110".to_string())
        ));
        
        let metrics_handler = Arc::new(MetricsServiceHandler::new());
        let config_handler = Arc::new(ConfigServiceHandler::new());
        
        // 创建核心通信Handler
        let communication_handler = Arc::new(
            GatewayHandler::new(
                signaling_client.clone(),
                storage_client.clone(),
                push_client.clone(),
            )
        );
        
        Ok(Self {
            message_handler,
            session_handler,
            user_handler,
            push_handler,
            tenant_handler,
            hook_handler,
            metrics_handler,
            config_handler,
            communication_handler,
        })
    }
}

