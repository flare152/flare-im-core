mod application;
mod config;
mod domain;
mod infrastructure;
mod interface;
mod util;

use flare_im_core::{load_config, register_service};
use flare_server_core::error::{ErrorBuilder, ErrorCode, Result};
use interface::grpc::SignalingOnlineServer;
use std::net::SocketAddr;
use tonic::transport::Server;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let app_config = load_config(Some("config"));
    let runtime_config = app_config.base().clone();

    register_service(&runtime_config, "signaling-online")
        .await
        .map_err(|err| {
            ErrorBuilder::new(ErrorCode::ServiceUnavailable, "service registration failed")
                .details(err.to_string())
                .build_error()
        })?;
    info!("✅ 服务已注册到注册中心");

    let addr: SocketAddr = format!(
        "{}:{}",
        runtime_config.server.address, runtime_config.server.port
    )
    .parse()
    .map_err(|err: std::net::AddrParseError| {
        ErrorBuilder::new(ErrorCode::ConfigurationError, "invalid server address")
            .details(err.to_string())
            .build_error()
    })?;

    let service = SignalingOnlineServer::new(runtime_config.clone()).await?;

    info!("Starting Signaling Online Service on {}", addr);

    Server::builder()
        .add_service(
            flare_proto::signaling::signaling_service_server::SignalingServiceServer::new(service),
        )
        .serve(addr)
        .await
        .map_err(|err: tonic::transport::Error| {
            ErrorBuilder::new(ErrorCode::ServiceUnavailable, "grpc server failed")
                .details(err.to_string())
                .build_error()
        })?;

    Ok(())
}
