//! # Hook配置仓储实现
//!
//! 提供Hook配置数据的持久化访问接口。

use anyhow::Result;
use async_trait::async_trait;
use sqlx::{PgPool, FromRow};
use tracing::debug;
use serde_json;
use chrono::{DateTime, Utc};

/// Hook配置仓储接口
#[async_trait]
pub trait HookConfigRepository: Send + Sync {
    /// 获取租户的所有Hook配置
    async fn get_hook_configs(&self, tenant_id: &str) -> Result<Vec<HookConfigRecord>>;
    
    /// 获取指定Hook配置
    async fn get_hook_config(&self, hook_id: &str) -> Result<Option<HookConfigRecord>>;
    
    /// 创建Hook配置
    async fn create_hook_config(&self, config: &HookConfigRecord) -> Result<()>;
    
    /// 更新Hook配置
    async fn update_hook_config(&self, config: &HookConfigRecord) -> Result<()>;
    
    /// 删除Hook配置
    async fn delete_hook_config(&self, hook_id: &str) -> Result<()>;
    
    /// 启用/禁用Hook配置
    async fn set_hook_status(&self, hook_id: &str, enabled: bool) -> Result<()>;
}

/// Hook配置记录
#[derive(Debug, Clone, FromRow)]
pub struct HookConfigRecord {
    pub hook_id: String,
    pub name: String,
    pub hook_type: String,
    pub tenant_id: String,
    pub priority: i32,
    pub enabled: bool,
    pub transport: serde_json::Value,
    pub selector: serde_json::Value,
    pub retry_policy: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Hook配置仓储实现
pub struct HookConfigRepositoryImpl {
    pool: PgPool,
}

impl HookConfigRepositoryImpl {
    /// 创建Hook配置仓储
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl HookConfigRepository for HookConfigRepositoryImpl {
    async fn get_hook_configs(&self, tenant_id: &str) -> Result<Vec<HookConfigRecord>> {
        let records = sqlx::query_as::<_, HookConfigRecord>(
            r#"
            SELECT 
                hook_id, name, hook_type, tenant_id, priority, enabled,
                transport, selector, retry_policy, created_at, updated_at
            FROM hook_configs
            WHERE tenant_id = $1
            ORDER BY priority DESC
            "#
        )
        .bind(tenant_id)
        .fetch_all(&self.pool)
        .await?;
        
        debug!(tenant_id = %tenant_id, count = records.len(), "Fetched hook configs");
        Ok(records)
    }
    
    async fn get_hook_config(&self, hook_id: &str) -> Result<Option<HookConfigRecord>> {
        let record = sqlx::query_as::<_, HookConfigRecord>(
            r#"
            SELECT 
                hook_id, name, hook_type, tenant_id, priority, enabled,
                transport, selector, retry_policy, created_at, updated_at
            FROM hook_configs
            WHERE hook_id = $1
            "#
        )
        .bind(hook_id)
        .fetch_optional(&self.pool)
        .await?;
        
        Ok(record)
    }
    
    async fn create_hook_config(&self, config: &HookConfigRecord) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO hook_configs (
                hook_id, name, hook_type, tenant_id, priority, enabled,
                transport, selector, retry_policy, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            "#,
        )
        .bind(&config.hook_id)
        .bind(&config.name)
        .bind(&config.hook_type)
        .bind(&config.tenant_id)
        .bind(config.priority)
        .bind(config.enabled)
        .bind(&config.transport)
        .bind(&config.selector)
        .bind(&config.retry_policy)
        .bind(config.created_at)
        .bind(config.updated_at)
        .execute(&self.pool)
        .await?;
        
        debug!(hook_id = %config.hook_id, "Created hook config");
        Ok(())
    }
    
    async fn update_hook_config(&self, config: &HookConfigRecord) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE hook_configs
            SET 
                name = $2, hook_type = $3, priority = $4, enabled = $5,
                transport = $6, selector = $7, retry_policy = $8, updated_at = $9
            WHERE hook_id = $1
            "#,
        )
        .bind(&config.hook_id)
        .bind(&config.name)
        .bind(&config.hook_type)
        .bind(config.priority)
        .bind(config.enabled)
        .bind(&config.transport)
        .bind(&config.selector)
        .bind(&config.retry_policy)
        .bind(config.updated_at)
        .execute(&self.pool)
        .await?;
        
        debug!(hook_id = %config.hook_id, "Updated hook config");
        Ok(())
    }
    
    async fn delete_hook_config(&self, hook_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM hook_configs WHERE hook_id = $1")
            .bind(hook_id)
            .execute(&self.pool)
            .await?;
        
        debug!(hook_id = %hook_id, "Deleted hook config");
        Ok(())
    }
    
    async fn set_hook_status(&self, hook_id: &str, enabled: bool) -> Result<()> {
        sqlx::query("UPDATE hook_configs SET enabled = $2, updated_at = NOW() WHERE hook_id = $1")
            .bind(hook_id)
            .bind(enabled)
            .execute(&self.pool)
            .await?;
        
        debug!(hook_id = %hook_id, enabled, "Updated hook status");
        Ok(())
    }
}

