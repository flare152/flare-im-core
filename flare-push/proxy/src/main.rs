use std::sync::Arc;

use anyhow::Result;
use flare_im_core::{load_config, register_service};
use flare_proto::push::push_service_server::PushServiceServer;
use flare_push_proxy::application::commands::PushCommandService;
use flare_push_proxy::domain::repositories::PushEventPublisher;
use flare_push_proxy::infrastructure::config::PushProxyConfig;
use flare_push_proxy::infrastructure::messaging::kafka_publisher::KafkaPushEventPublisher;
use flare_push_proxy::interfaces::grpc::handler::PushGrpcHandler;
use flare_push_proxy::interfaces::grpc::server::PushProxyGrpcServer;
use tonic::transport::Server;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let app_config = load_config(Some("config"));
    let service_cfg = app_config.push_proxy_service();
    let runtime_config =
        app_config.compose_service_config(&service_cfg.runtime, "flare-push-proxy");
    let service_type = runtime_config.service.name.clone();
    let proxy_config = Arc::new(PushProxyConfig::from_app_config(app_config));

    let _registry = register_service(&runtime_config, &service_type).await?;
    info!("✅ 服务已注册到注册中心");

    let addr = format!(
        "{}:{}",
        runtime_config.server.address, runtime_config.server.port
    )
    .parse()?;

    let publisher = build_publisher(proxy_config.clone())?;
    let command_service = Arc::new(PushCommandService::new(publisher));
    let handler = Arc::new(PushGrpcHandler::new(command_service));
    let grpc_server = PushProxyGrpcServer::new(handler);

    info!("Starting Push Proxy Service on {}", addr);

    Server::builder()
        .add_service(PushServiceServer::new(grpc_server))
        .serve(addr)
        .await?;

    Ok(())
}

fn build_publisher(config: Arc<PushProxyConfig>) -> anyhow::Result<Arc<dyn PushEventPublisher>> {
    Ok(Arc::new(KafkaPushEventPublisher::new(config)?))
}
