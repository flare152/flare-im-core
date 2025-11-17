//! # 数据库连接管理
//!
//! 提供PostgreSQL数据库连接池的创建和管理。

use anyhow::Result;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::time::Duration;
use tracing::info;

/// 创建PostgreSQL连接池
pub async fn create_db_pool(database_url: &str) -> Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(100)
        .min_connections(10)
        .acquire_timeout(Duration::from_secs(30))
        .idle_timeout(Duration::from_secs(600))
        .max_lifetime(Duration::from_secs(1800))
        .connect(database_url)
        .await?;
    
    info!("Database connection pool created");
    
    // 测试连接
    sqlx::query("SELECT 1")
        .execute(&pool)
        .await?;
    
    info!("Database connection test successful");
    
    Ok(pool)
}

/// 从环境变量创建数据库连接池
pub async fn create_db_pool_from_env() -> Result<PgPool> {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://localhost/flare_im".to_string());
    
    create_db_pool(&database_url).await
}

