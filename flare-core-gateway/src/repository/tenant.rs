//! # 租户仓储实现
//!
//! 提供租户数据的持久化访问接口。

use anyhow::Result;
use async_trait::async_trait;
use sqlx::PgPool;
use tracing::debug;

// Re-export TenantRepository trait
pub use crate::middleware::tenant::TenantRepository;

/// 租户仓储实现
pub struct TenantRepositoryImpl {
    pool: PgPool,
}

impl TenantRepositoryImpl {
    /// 创建租户仓储
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TenantRepository for TenantRepositoryImpl {
    async fn tenant_exists(&self, tenant_id: &str) -> Result<bool> {
        let result = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM tenants WHERE tenant_id = $1 AND status = 'active')"
        )
        .bind(tenant_id)
        .fetch_one(&self.pool)
        .await?;
        
        debug!(tenant_id = %tenant_id, exists = result, "Checked tenant existence");
        Ok(result)
    }
    
    async fn is_tenant_enabled(&self, tenant_id: &str) -> Result<bool> {
        let result = sqlx::query_scalar::<_, bool>(
            "SELECT status = 'active' FROM tenants WHERE tenant_id = $1"
        )
        .bind(tenant_id)
        .fetch_optional(&self.pool)
        .await?
        .unwrap_or(false);
        
        debug!(tenant_id = %tenant_id, enabled = result, "Checked tenant status");
        Ok(result)
    }
}

