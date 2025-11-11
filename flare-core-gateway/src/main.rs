mod config;
mod error;
mod handler;
mod infrastructure;
mod server;
mod transform;

use anyhow::{anyhow, Result};
use config::GatewayConfig;
use flare_im_core::error::FlareError;
use flare_im_core::{load_config, register_service};
use server::CommunicationCoreGatewayServer;
use std::net::SocketAddr;
use tonic::transport::Server;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let app_config = load_config(Some("config"));
    let runtime_config = app_config.base().clone();

    let addr: SocketAddr = format!(
        "{}:{}",
        runtime_config.server.address, runtime_config.server.port
    )
    .parse()?;

    let gateway_config = GatewayConfig::from_env();

    let _registry: Option<_> = register_service(&runtime_config, "communication-core-gateway")
        .await
        .map_err(|err: FlareError| anyhow!(err.to_string()))?;

    let service = CommunicationCoreGatewayServer::new(gateway_config).await?;

    info!("Starting Communication Core Gateway on {}", addr);

    Server::builder()
        .add_service(
            flare_proto::communication_core::communication_core_server::CommunicationCoreServer::new(
                service,
            ),
        )
        .serve(addr)
        .await
        .map_err(|err| anyhow!(err.to_string()))?;

    Ok(())
}
