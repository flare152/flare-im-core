mod application;
mod domain;
mod handler;
mod infrastructure;
mod server;

use flare_server_core::Config;
use server::StorageReaderServer;
use tonic::transport::Server;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let config = Config::load_from_file("config/reader.toml").unwrap_or_else(|_| {
        info!("Using default config");
        default_config()
    });

    let addr = format!("{}:{}", config.server.address, config.server.port).parse()?;

    let service = StorageReaderServer::new(config).await?;

    info!("Starting Storage Reader Service on {}", addr);

    Server::builder()
        .add_service(
            flare_proto::storage::storage_service_server::StorageServiceServer::new(service),
        )
        .serve(addr)
        .await?;

    Ok(())
}

fn default_config() -> Config {
    Config {
        service: flare_server_core::ServiceConfig {
            name: "flare-storage-reader".to_string(),
            version: "0.1.0".to_string(),
        },
        server: flare_server_core::ServerConfig {
            address: "0.0.0.0".to_string(),
            port: 50082,
        },
        registry: None,
        mesh: None,
        storage: None,
    }
}
