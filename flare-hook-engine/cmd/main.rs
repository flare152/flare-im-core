//! # Flare Hook Engine 入口
//!
//! Hook引擎的启动入口

use anyhow::Result;
use flare_hook_engine::service::bootstrap::{ApplicationBootstrap, HookEngineConfig};
use flare_hook_engine::domain::models::ExecutionMode;
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

    // 从环境变量读取配置
    let database_url = std::env::var("DATABASE_URL")
        .ok()
        .or_else(|| {
            // 默认使用 docker-compose 中的 PostgreSQL 配置
            Some("postgresql://flare:flare123@localhost:25432/flare".to_string())
        });
    
    let config_center_endpoint = std::env::var("CONFIG_CENTER_ENDPOINT")
        .ok()
        .or_else(|| {
            // 默认使用 docker-compose 中的 etcd 配置
            Some("etcd://localhost:22379".to_string())
        });
    
    let tenant_id = std::env::var("TENANT_ID").ok();
    
    let config_file = std::env::var("CONFIG_FILE")
        .ok()
        .map(|s| std::path::PathBuf::from(s));

    // 创建Hook引擎配置
    let config = HookEngineConfig {
        config_file,
        database_url,
        config_center_endpoint,
        tenant_id,
        execution_mode: ExecutionMode::Sequential,
        refresh_interval_secs: 60,
    };

    tracing::info!("Starting Hook Engine with config: {:?}", config);

    // 启动应用
    ApplicationBootstrap::run(config).await
}

