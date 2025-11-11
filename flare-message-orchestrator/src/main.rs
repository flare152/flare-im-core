mod application;
mod domain;
mod handler;
mod infrastructure;
mod server;

use crate::infrastructure::config::MessageOrchestratorConfig;
use anyhow::{Result, anyhow};
use flare_im_core::error::FlareError;
use flare_im_core::{load_config, register_service};
use server::MessageOrchestratorServer;
use std::net::SocketAddr;
use std::sync::Arc;
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

    let _registry: Option<_> = register_service(&runtime_config, "message-orchestrator")
        .await
        .map_err(|err: FlareError| anyhow!(err.to_string()))?;

    let orchestrator_config = MessageOrchestratorConfig::from_sources(Some(app_config));
    let service = MessageOrchestratorServer::new(Arc::new(orchestrator_config)).await?;

    info!("Starting Message Orchestrator Service on {}", addr);

    Server::builder()
        .add_service(
            flare_proto::storage::storage_service_server::StorageServiceServer::new(service),
        )
        .serve(addr)
        .await
        .map_err(|err| anyhow!(err.to_string()))?;

    Ok(())
}
