use std::sync::Arc;

use flare_im_core::load_config;
use flare_push_server::infrastructure::config::PushServerConfig;
use flare_push_server::server::PushServer;
use flare_server_core::error::Result;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let app_config = load_config(Some("config"));
    let server_config = PushServerConfig::from_app_config(app_config);

    let server = Arc::new(PushServer::new(server_config).await?);

    info!("Starting Push Server");

    server.run().await
}
