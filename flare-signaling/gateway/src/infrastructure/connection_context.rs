//! 连接上下文提取模块
//!
//! 从连接信息中提取上下文（租户ID、用户ID等）并设置到请求中

use flare_server_core::context::{RequestContext, TenantContext};
use std::collections::HashMap;

/// 连接上下文键（用于存储在 ConnectionInfo.metadata 中）
pub const METADATA_KEY_TENANT_ID: &str = "tenant_id";
pub const METADATA_KEY_USER_ID: &str = "user_id";
pub const METADATA_KEY_DEVICE_ID: &str = "device_id";

/// 从连接信息的 metadata 中提取租户ID
pub fn extract_tenant_id_from_metadata(metadata: &HashMap<String, String>) -> Option<String> {
    metadata.get(METADATA_KEY_TENANT_ID).cloned()
}

/// 从连接信息的 metadata 中提取用户ID
pub fn extract_user_id_from_metadata(metadata: &HashMap<String, String>) -> Option<String> {
    metadata.get(METADATA_KEY_USER_ID).cloned()
}

/// 从连接信息的 metadata 中提取设备ID
pub fn extract_device_id_from_metadata(metadata: &HashMap<String, String>) -> Option<String> {
    metadata.get(METADATA_KEY_DEVICE_ID).cloned()
}

/// 构建 TenantContext（从连接 metadata 中提取，如果没有则使用默认值）
pub fn build_tenant_context_from_metadata(
    metadata: &HashMap<String, String>,
    default_tenant_id: &str,
) -> TenantContext {
    let tenant_id = extract_tenant_id_from_metadata(metadata).unwrap_or_else(|| {
        default_tenant_id.to_string()
    });
    
    TenantContext::new(&tenant_id)
        .with_business_type("im")
}

/// 构建 RequestContext（从连接 metadata 中提取用户ID等信息）
pub fn build_request_context_from_metadata(
    metadata: &HashMap<String, String>,
    user_id: Option<&str>,
) -> RequestContext {
    let user_id = user_id
        .map(|s| s.to_string())
        .or_else(|| extract_user_id_from_metadata(metadata));
    
    let device_id = extract_device_id_from_metadata(metadata);
    
    let mut req_ctx = RequestContext::default();
    
    if let Some(user_id) = user_id {
        req_ctx.actor = Some(flare_server_core::context::ActorContext {
            actor_id: user_id.clone(),
            actor_type: flare_server_core::context::ActorType::User,
            roles: vec![],
            attributes: std::collections::HashMap::new(),
        });
    }
    
    if let Some(device_id) = device_id {
        req_ctx.device = Some(flare_server_core::context::DeviceContext {
            device_id,
            platform: "unknown".to_string(),
            model: String::new(),
            os_version: String::new(),
            app_version: String::new(),
            locale: String::new(),
            timezone: String::new(),
            ip_address: String::new(),
            priority: flare_server_core::context::DevicePriority::Normal,
            token_version: 0,
            connection_quality: None,
            attributes: std::collections::HashMap::new(),
        });
    }
    
    req_ctx
}

