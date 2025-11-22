//! # 租户上下文中间件
//!
//! 提供租户上下文提取和验证功能。

use anyhow::Result;
use flare_proto::TenantContext;
use tracing::debug;

use crate::interface::middleware::auth::TokenClaims;
use flare_server_core::error::{ErrorBuilder, ErrorCode, FlareError};

/// 租户上下文中间件
pub struct TenantMiddleware;

impl TenantMiddleware {
    /// 从Token Claims提取租户上下文
    pub fn extract_from_claims(claims: &TokenClaims) -> TenantContext {
        TenantContext {
            tenant_id: claims.tenant_id.clone(),
            ..Default::default()
        }
    }
    
    /// 从请求Metadata提取租户上下文（备用方法）
    pub fn extract_from_metadata(_metadata: &tonic::metadata::MetadataMap) -> Option<TenantContext> {
        // TODO: 实现从Metadata提取租户上下文
        // 可以支持x-tenant-id header等
        None
    }
    
    /// 验证租户上下文
    pub async fn validate_tenant_context(
        _tenant_context: &TenantContext,
        _tenant_repository: Option<&dyn TenantRepository>,
    ) -> Result<()> {
        // TODO: 如果提供了tenant_repository，验证租户是否存在和启用
        // 目前先简单返回成功
        debug!("Tenant context validated");
        Ok(())
    }
}

/// 租户仓储接口（用于验证租户）
#[async_trait::async_trait]
pub trait TenantRepository: Send + Sync {
    /// 检查租户是否存在
    async fn tenant_exists(&self, tenant_id: &str) -> Result<bool>;
    
    /// 检查租户是否启用
    async fn is_tenant_enabled(&self, tenant_id: &str) -> Result<bool>;
}

