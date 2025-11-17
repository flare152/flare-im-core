use std::sync::Arc;

use anyhow::Result;
use flare_proto::TenantContext;
// 注意：communication_core.proto 已删除
// 业务系统应该使用 AccessGateway 接口推送消息
// use flare_proto::communication_core::communication_core_server::CommunicationCore;
// use flare_proto::communication_core::*;
use tonic::{Request, Response, Status};

use crate::config::GatewayConfig;
use crate::handler::{GatewayHandler, gateway::tenant_id_label};
use crate::infrastructure::{GrpcPushClient, GrpcSignalingClient, GrpcStorageClient};
use tracing::debug;

pub struct CommunicationCoreGatewayServer {
    handler: Arc<GatewayHandler>,
}

impl CommunicationCoreGatewayServer {
    pub async fn new(config: GatewayConfig) -> Result<Self> {
        let signaling = GrpcSignalingClient::new(config.signaling_endpoint.clone());
        let storage = GrpcStorageClient::new(config.message_endpoint.clone());
        let push = GrpcPushClient::new(config.push_endpoint.clone());

        let handler = Arc::new(GatewayHandler::new(signaling, storage, push));
        Ok(Self { handler })
    }
}

fn request_span<'a>(method: &'static str, tenant: Option<&'a TenantContext>) -> tracing::Span {
    let tenant_label = tenant_id_label(tenant);
    // 使用format!来避免常量问题
    tracing::info_span!("request", method = method, tenant = %tenant_label)
}

// 注意：communication_core.proto 已删除，相关实现已注释
// 如果需要统一网关功能，可以聚合多个服务的gRPC接口
// #[tonic::async_trait]
// impl CommunicationCore for CommunicationCoreGatewayServer {
//     所有方法实现已注释，因为 communication_core.proto 已删除
//     如果需要统一网关功能，可以聚合多个服务的gRPC接口
// }
