use anyhow::Result;
use flare_im_core::load_config;
use flare_media::ApplicationBootstrap;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

/// 初始化日志系统
fn init_tracing() {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::DEBUG)
        .finish();
    let _ = tracing::subscriber::set_global_default(subscriber);
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    // 加载配置
    let app_config = load_config(Some("config"));

    // 创建应用上下文并启动服务器
    ApplicationBootstrap::run(app_config).await
}
