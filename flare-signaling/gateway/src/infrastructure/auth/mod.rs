//! 认证模块
//!
//! 提供 token 认证功能

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use flare_core::common::device::DeviceInfo;
use flare_core::common::error::Result;
use flare_core::server::auth::{AuthResult, Authenticator};
use flare_server_core::TokenService;
use tracing::{debug, instrument, warn};

/// Token 认证器
///
/// 验证客户端提供的 token，提取用户ID
pub struct TokenAuthenticator {
    token_service: Arc<TokenService>,
}

impl TokenAuthenticator {
    pub fn new(token_service: Arc<TokenService>) -> Self {
        Self { token_service }
    }

    /// 验证 token（调用核心 TokenService）
    ///
    /// 返回完整的 TokenClaims，如果验证失败则返回 None
    fn verify_token(&self, token: &str) -> Option<flare_server_core::TokenClaims> {
        match self.token_service.validate_token(token) {
            Ok(claims) => Some(claims),
            Err(err) => {
                warn!(?err, "Token validation failed");
                None
            }
        }
    }

    /// 获取 token 预览（用于日志记录）
    fn token_preview(&self, token: &str) -> String {
        if token.len() > 12 {
            format!("{}...", &token[..12])
        } else {
            token.to_string()
        }
    }
}

#[async_trait]
impl Authenticator for TokenAuthenticator {
    #[instrument(skip(self), fields(connection_id, token_len = token.len()))]
    async fn authenticate(
        &self,
        token: &str,
        connection_id: &str,
        device_info: Option<&DeviceInfo>,
        _metadata: Option<&HashMap<String, Vec<u8>>>,
    ) -> Result<AuthResult> {
        // 记录设备信息
        if let Some(device) = device_info {
            tracing::Span::current().record("device_id", &device.device_id);
            tracing::Span::current().record("platform", &device.platform.as_str());
        }

        debug!(
            connection_id = %connection_id,
            token_preview = %self.token_preview(token),
            device_id = ?device_info.map(|d| d.device_id.clone()),
            "验证 token"
        );

        match self.verify_token(token) {
            Some(claims) => {
                let user_id = claims.sub.clone();
                
                // 构建用户元数据（包含 tenant_id、device_id 等信息）
                let mut user_metadata = std::collections::HashMap::new();
                user_metadata.insert("user_id".to_string(), user_id.clone());
                if let Some(tenant_id) = claims.tenant_id {
                    user_metadata.insert("tenant_id".to_string(), tenant_id);
                }
                if let Some(device_id) = claims.device_id {
                    user_metadata.insert("device_id".to_string(), device_id);
                }
                
                debug!(
                    connection_id = %connection_id,
                    user_id = %user_id,
                    tenant_id = ?user_metadata.get("tenant_id"),
                    "✅ Token 验证成功"
                );
                Ok(AuthResult::success_with_metadata(
                    Some(user_id),
                    user_metadata,
                ))
            }
            None => {
                warn!(
                    connection_id = %connection_id,
                    token_preview = %self.token_preview(token),
                    "❌ Token 验证失败"
                );
                Ok(AuthResult::failure("Token 无效或已过期".to_string()))
            }
        }
    }
}
