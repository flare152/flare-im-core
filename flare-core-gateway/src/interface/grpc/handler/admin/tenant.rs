//! # TenantService Handler
//!
//! 提供租户上下文验证和配置查询能力。

use std::sync::Arc;

use anyhow::Result;
// TODO: 等待admin.proto生成Rust代码后启用
// use flare_proto::admin::tenant_service_server::TenantService;
// use flare_proto::admin::{
//     GetTenantConfigRequest, GetTenantConfigResponse, ValidateTenantContextRequest,
//     ValidateTenantContextResponse,
// };
use tonic::{Request, Response, Status};
use tracing::debug;

use crate::interface::interceptor::{extract_claims, extract_tenant_context};
use crate::interface::middleware::tenant::TenantRepository;
use crate::domain::repository::tenant::TenantRepositoryImpl;

/// TenantService Handler实现
pub struct TenantServiceHandler {
    /// 租户仓储
    tenant_repository: Arc<TenantRepositoryImpl>,
}

impl TenantServiceHandler {
    /// 创建TenantService Handler
    pub fn new(tenant_repository: Arc<TenantRepositoryImpl>) -> Self {
        Self { tenant_repository }
    }
    
    /// 验证租户上下文（内部方法）
    pub async fn validate_tenant_context_internal(
        &self,
        tenant_id: &str,
    ) -> Result<(bool, Option<String>, Option<String>)> {
        // 检查租户是否存在
        let exists = self.tenant_repository
            .tenant_exists(tenant_id)
            .await?;

        if !exists {
            return Ok((false, Some("Tenant does not exist".to_string()), None));
        }

        // 检查租户是否启用
        let enabled = self.tenant_repository
            .is_tenant_enabled(tenant_id)
            .await?;

        Ok((
            enabled,
            if enabled {
                None
            } else {
                Some("Tenant is disabled".to_string())
            },
            Some(if enabled { "active".to_string() } else { "suspended".to_string() }),
        ))
    }
}

// TODO: 等待admin.proto生成Rust代码后启用
/*
#[tonic::async_trait]
impl TenantService for TenantServiceHandler {
    /// 验证租户上下文
    async fn validate_tenant_context(
        &self,
        request: Request<ValidateTenantContextRequest>,
    ) -> Result<Response<ValidateTenantContextResponse>, Status> {
        let req = request.into_inner();
        let _claims = extract_claims(&request)
            .ok_or_else(|| Status::unauthenticated("Missing authentication"))?;

        debug!(tenant_id = %req.tenant_id, "ValidateTenantContext request");

        let (valid, reason, status) = self.validate_tenant_context_internal(&req.tenant_id)
            .await
            .map_err(|e| Status::internal(format!("Failed to validate tenant: {}", e)))?;

        Ok(Response::new(ValidateTenantContextResponse {
            valid,
            reason,
            status,
        }))
    }

    /// 获取租户配置
    async fn get_tenant_config(
        &self,
        request: Request<GetTenantConfigRequest>,
    ) -> Result<Response<GetTenantConfigResponse>, Status> {
        let req = request.into_inner();
        let _claims = extract_claims(&request)
            .ok_or_else(|| Status::unauthenticated("Missing authentication"))?;

        debug!(tenant_id = %req.tenant_id, "GetTenantConfig request");

        // TODO: 从数据库查询租户配置
        // 目前返回空配置
        Ok(Response::new(GetTenantConfigResponse {
            config: None,
            quota: None,
            status: None,
        }))
    }
}
*/

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::repository::tenant::TenantRepository;
    use sqlx::PgPool;
    use std::sync::Arc;

    // 注意：这些测试需要数据库连接，在实际测试中应该使用测试数据库
    // 这里只是展示测试结构
    
    #[tokio::test]
    #[ignore] // 需要数据库连接，默认忽略
    async fn test_validate_tenant_context_existing() {
        // TODO: 设置测试数据库连接
        // let pool = create_test_db_pool().await;
        // let repo = Arc::new(TenantRepositoryImpl::new(pool));
        // let handler = TenantServiceHandler::new(repo);
        // 
        // let (valid, reason, status) = handler.validate_tenant_context_internal("test-tenant")
        //     .await
        //     .unwrap();
        // 
        // assert!(valid);
        // assert_eq!(status, Some("active".to_string()));
    }
    
    #[tokio::test]
    #[ignore]
    async fn test_validate_tenant_context_nonexistent() {
        // TODO: 测试不存在的租户
    }
}
