//! # gRPC拦截器
//!
//! 提供统一的请求拦截和处理功能，集成认证、授权、限流等中间件。

use std::sync::Arc;

use anyhow::Result;
use tonic::{Request, Status};

use crate::middleware::{AuthMiddleware, RateLimitMiddleware, RbacMiddleware, TenantMiddleware};
use crate::repository::tenant::TenantRepositoryImpl;

pub mod auth_interceptor;

pub use auth_interceptor::AuthInterceptorService;

impl Clone for GatewayInterceptor {
    fn clone(&self) -> Self {
        Self {
            auth_middleware: Arc::clone(&self.auth_middleware),
            tenant_middleware: Arc::clone(&self.tenant_middleware),
            rbac_middleware: Arc::clone(&self.rbac_middleware),
            rate_limit_middleware: Arc::clone(&self.rate_limit_middleware),
            tenant_repository: self.tenant_repository.as_ref().map(Arc::clone),
        }
    }
}

/// 请求上下文（在拦截器中传递）
#[derive(Debug, Clone)]
pub struct RequestContext {
    /// Token Claims
    pub claims: Option<crate::middleware::auth::TokenClaims>,
    /// 租户上下文
    pub tenant_context: Option<flare_proto::TenantContext>,
    /// 客户端IP
    pub client_ip: Option<String>,
}

impl RequestContext {
    pub fn new() -> Self {
        Self {
            claims: None,
            tenant_context: None,
            client_ip: None,
        }
    }
}

/// 统一网关拦截器
pub struct GatewayInterceptor {
    /// 认证中间件
    auth_middleware: Arc<AuthMiddleware>,
    /// 租户中间件
    tenant_middleware: Arc<TenantMiddleware>,
    /// RBAC中间件
    rbac_middleware: Arc<RbacMiddleware>,
    /// 限流中间件
    rate_limit_middleware: Arc<RateLimitMiddleware>,
    /// 租户仓储（可选）
    tenant_repository: Option<Arc<TenantRepositoryImpl>>,
}

impl GatewayInterceptor {
    /// 创建拦截器
    pub fn new(
        auth_middleware: Arc<AuthMiddleware>,
        tenant_middleware: Arc<TenantMiddleware>,
        rbac_middleware: Arc<RbacMiddleware>,
        rate_limit_middleware: Arc<RateLimitMiddleware>,
        tenant_repository: Option<Arc<TenantRepositoryImpl>>,
    ) -> Self {
        Self {
            auth_middleware,
            tenant_middleware,
            rbac_middleware,
            rate_limit_middleware,
            tenant_repository,
        }
    }
    
    /// 从环境变量创建拦截器
    pub fn from_env() -> Result<Self> {
        let auth_middleware = Arc::new(AuthMiddleware::from_env()?);
        let tenant_middleware = Arc::new(TenantMiddleware);
        let rbac_middleware = Arc::new(RbacMiddleware);
        let rate_limit_middleware = Arc::new(RateLimitMiddleware::default());
        
        Ok(Self::new(
            auth_middleware,
            tenant_middleware,
            rbac_middleware,
            rate_limit_middleware,
            None, // 租户仓储可选
        ))
    }
    
    /// 处理请求（认证、授权、限流）
    pub async fn process_request(&self, mut request: Request<()>) -> Result<Request<()>, Status> {
        let metadata = request.metadata();
        
        // 1. 认证：提取和验证Token
        let claims = match self.auth_middleware.authenticate(metadata) {
            Ok(claims) => claims,
            Err(e) => {
                return Err(Status::unauthenticated(format!("Authentication failed: {}", e)));
            }
        };
        
        // 2. 租户上下文提取
        let tenant_context = TenantMiddleware::extract_from_claims(&claims);
        
        // 3. 租户验证（如果提供了仓储）
        if let Some(ref repo) = self.tenant_repository {
            if let Err(e) = TenantMiddleware::validate_tenant_context(&tenant_context, Some(repo.as_ref())).await {
                return Err(Status::invalid_argument(format!("Tenant validation failed: {}", e)));
            }
        }
        
        // 4. 限流检查
        let client_ip = Self::extract_client_ip(metadata);
        if let Err(e) = self.rate_limit_middleware.check_rate_limit(&claims, client_ip.as_deref()).await {
            return Err(Status::resource_exhausted(format!("Rate limit exceeded: {}", e)));
        }
        
        // 5. 将Claims和租户上下文注入到请求扩展中
        request.extensions_mut().insert(claims.clone());
        request.extensions_mut().insert(tenant_context.clone());
        
        Ok(request)
    }
    
    /// 提取客户端IP
    fn extract_client_ip(metadata: &tonic::metadata::MetadataMap) -> Option<String> {
        // 尝试从x-forwarded-for header提取
        if let Some(forwarded) = metadata.get("x-forwarded-for") {
            if let Ok(ip_str) = forwarded.to_str() {
                // x-forwarded-for可能包含多个IP，取第一个
                return ip_str.split(',').next().map(|s| s.trim().to_string());
            }
        }
        
        // 尝试从x-real-ip header提取
        if let Some(real_ip) = metadata.get("x-real-ip") {
            if let Ok(ip_str) = real_ip.to_str() {
                return Some(ip_str.to_string());
            }
        }
        
        None
    }
}

/// 从请求扩展中提取Claims
pub fn extract_claims<T>(request: &Request<T>) -> Option<&crate::middleware::auth::TokenClaims> {
    request.extensions().get::<crate::middleware::auth::TokenClaims>()
}

/// 从请求扩展中提取租户上下文
pub fn extract_tenant_context<T>(request: &Request<T>) -> Option<&flare_proto::TenantContext> {
    request.extensions().get::<flare_proto::TenantContext>()
}

