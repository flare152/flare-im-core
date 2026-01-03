//! Context 工具函数
//!
//! 提供从 gRPC Request 中提取 Context 的便捷函数

use flare_server_core::context::Context;
use flare_server_core::middleware::extract_context;
use tonic::{Request, Status};

/// 从 gRPC 请求中提取 Context（必需版本）
///
/// 如果 Context 不存在，返回错误。
///
/// # 示例
///
/// ```rust,no_run
/// use flare_im_core::utils::context::require_context;
/// use tonic::Request;
///
/// async fn my_handler(req: Request<MyRequest>) -> Result<Response<MyResponse>, Status> {
///     let ctx = require_context(&req)?;
///     let tenant_id = ctx.tenant_id().ok_or_else(|| {
///         Status::invalid_argument("Tenant ID is required")
///     })?;
///     // ...
/// }
/// ```
pub fn require_context<T>(req: &Request<T>) -> Result<Context, Status> {
    extract_context(req)
}

/// 从 gRPC 请求中提取 Context（可选版本）
///
/// 如果 Context 不存在，返回 None。
pub fn extract_context_opt<T>(req: &Request<T>) -> Option<Context> {
    extract_context(req).ok()
}

/// 从 Context 中提取租户ID（必需版本）
///
/// 优先从 TenantContext 中获取，如果没有则从 tenant_id 字段获取。
pub fn require_tenant_id_from_context(ctx: &Context) -> Result<String, Status> {
    // 优先从 TenantContext 中获取
    if let Some(tenant) = ctx.tenant() {
        if !tenant.tenant_id.is_empty() {
            return Ok(tenant.tenant_id.clone());
        }
    }
    
    // 从 tenant_id 字段获取
    ctx.tenant_id()
        .map(|s| s.to_string())
        .ok_or_else(|| Status::invalid_argument("Tenant ID is required in context"))
}

/// 从 Context 中提取用户ID（必需版本）
///
/// 优先从 RequestContext.actor 中获取，如果没有则从 user_id 字段获取。
pub fn require_user_id_from_context(ctx: &Context) -> Result<String, Status> {
    // 优先从 RequestContext.actor 中获取
    if let Some(req_ctx) = ctx.request() {
        if let Some(actor) = &req_ctx.actor {
            if !actor.actor_id.is_empty() {
                return Ok(actor.actor_id.clone());
            }
        }
    }
    
    // 从 user_id 字段获取
    ctx.user_id()
        .map(|s| s.to_string())
        .ok_or_else(|| Status::invalid_argument("User ID is required in context"))
}

/// 从 Context 中提取会话ID（可选版本）
pub fn extract_session_id_from_context(ctx: &Context) -> Option<String> {
    ctx.session_id().map(|s| s.to_string())
}

/// 从 Context 中提取请求ID（必需版本）
pub fn require_request_id_from_context(ctx: &Context) -> Result<String, Status> {
    let request_id = ctx.request_id();
    if request_id.is_empty() {
        return Err(Status::invalid_argument("Request ID is required in context"));
    }
    Ok(request_id.to_string())
}

/// 从 gRPC 请求中提取租户ID（便捷函数，必需版本）
///
/// 自动从 Context 中提取租户ID。
pub fn require_tenant_id<T>(req: &Request<T>) -> Result<String, Status> {
    let ctx = require_context(req)?;
    require_tenant_id_from_context(&ctx)
}

/// 从 gRPC 请求中提取用户ID（便捷函数，必需版本）
///
/// 自动从 Context 中提取用户ID。
pub fn require_user_id<T>(req: &Request<T>) -> Result<String, Status> {
    let ctx = require_context(req)?;
    require_user_id_from_context(&ctx)
}

/// 从 gRPC 请求中提取会话ID（便捷函数，可选版本）
pub fn extract_session_id<T>(req: &Request<T>) -> Option<String> {
    let ctx = extract_context_opt(req)?;
    extract_session_id_from_context(&ctx)
}

/// 从 gRPC 请求中提取请求ID（便捷函数，必需版本）
pub fn require_request_id<T>(req: &Request<T>) -> Result<String, Status> {
    let ctx = require_context(req)?;
    require_request_id_from_context(&ctx)
}
