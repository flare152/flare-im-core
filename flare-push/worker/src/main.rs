use std::sync::Arc;

use flare_im_core::load_config;
use flare_push_worker::infrastructure::config::PushWorkerConfig;
use flare_push_worker::server::PushWorkerServer;
use flare_server_core::error::Result;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let app_config = load_config(Some("config"));
    let worker_config = PushWorkerConfig::from_app_config(app_config);

    let service = Arc::new(
        PushWorkerServer::new(worker_config, Default::default(), Default::default()).await?,
    );

    info!("Starting Push Worker");

    service.run().await
}
