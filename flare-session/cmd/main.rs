use flare_session::service::SessionServiceApp;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let app = SessionServiceApp::new().await?;

    info!(
        address = %app.address(),
        "Starting flare-session service"
    );

    app.run().await
}
