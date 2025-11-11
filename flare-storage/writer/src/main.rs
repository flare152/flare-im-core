mod application;
mod domain;
mod handler;
mod infrastructure;
mod server;

use server::StorageWriter;
use std::sync::Arc;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let service = Arc::new(StorageWriter::new().await?);

    info!("Starting Storage Writer");

    service.run().await?;

    Ok(())
}
