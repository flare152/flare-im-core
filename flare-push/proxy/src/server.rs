use std::sync::Arc;

use crate::application::commands::PushCommandService;
use crate::domain::repositories::PushEventPublisher;
use crate::infrastructure::config::PushProxyConfig;
use crate::infrastructure::messaging::kafka_publisher::KafkaPushEventPublisher;
use crate::interfaces::grpc::handler::PushGrpcHandler;
use crate::interfaces::grpc::server::PushProxyGrpcServer;
use flare_proto::push::push_service_server::PushServiceServer;
use tracing::info;

pub struct PushProxyServer {
    grpc_server: PushProxyGrpcServer,
}

impl PushProxyServer {
    pub async fn new(proxy_config: Arc<PushProxyConfig>) -> Result<Self, Box<dyn std::error::Error>> {
        let publisher = build_publisher(proxy_config.clone())?;
        let command_service = Arc::new(PushCommandService::new(publisher));
        let handler = Arc::new(PushGrpcHandler::new(command_service));
        let grpc_server = PushProxyGrpcServer::new(handler);

        info!("Push proxy server initialized");

        Ok(Self { grpc_server })
    }

    pub fn into_grpc_service(&self) -> PushServiceServer<PushProxyGrpcServer> {
        PushServiceServer::new(self.grpc_server.clone())
    }
}

fn build_publisher(
    config: Arc<PushProxyConfig>,
) -> anyhow::Result<Arc<dyn PushEventPublisher>> {
    Ok(Arc::new(KafkaPushEventPublisher::new(config)?))
}
