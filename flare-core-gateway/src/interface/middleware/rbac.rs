//! # RBAC中间件
//!
//! 提供基于角色的访问控制（RBAC）功能。

use anyhow::Result;
use tracing::debug;

use crate::interface::middleware::auth::TokenClaims;

/// RBAC中间件
#[derive(Clone)]
pub struct RbacMiddleware;

impl RbacMiddleware {
    /// 检查权限
    pub fn check_permission(claims: &TokenClaims, required_permission: &str) -> bool {
        // 检查用户是否有所需权限
        let has_permission = claims.permissions.contains(&required_permission.to_string());
        
        debug!(
            user_id = %claims.user_id,
            permission = %required_permission,
            has_permission,
            "Permission check"
        );
        
        has_permission
    }
    
    /// 检查角色
    pub fn check_role(claims: &TokenClaims, required_role: &str) -> bool {
        // 检查用户是否有所需角色
        let has_role = claims.roles.contains(&required_role.to_string());
        
        debug!(
            user_id = %claims.user_id,
            role = %required_role,
            has_role,
            "Role check"
        );
        
        has_role
    }
    
    /// 检查是否有任一权限
    pub fn check_any_permission(claims: &TokenClaims, required_permissions: &[&str]) -> bool {
        required_permissions.iter().any(|perm| {
            claims.permissions.contains(&perm.to_string())
        })
    }
    
    /// 检查是否有所有权限
    pub fn check_all_permissions(claims: &TokenClaims, required_permissions: &[&str]) -> bool {
        required_permissions.iter().all(|perm| {
            claims.permissions.contains(&perm.to_string())
        })
    }
}
