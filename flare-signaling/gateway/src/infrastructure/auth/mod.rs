//! 认证模块
//!
//! 提供 token 认证功能

use std::collections::HashMap;
use std::sync::Arc;

use flare_core::common::device::DeviceInfo;
use flare_core::common::error::{FlareError, Result};
use flare_core::server::auth::{AuthResult, Authenticator};
use async_trait::async_trait;
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
    /// 返回用户ID，如果验证失败则返回 None
    fn verify_token(&self, token: &str) -> Option<String> {
        match self.token_service.validate_token(token) {
            Ok(claims) => Some(claims.sub),
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
            Some(user_id) => {
                debug!(
                    connection_id = %connection_id,
                    user_id = %user_id,
                    "✅ Token 验证成功"
                );
                Ok(AuthResult::success(Some(user_id)))
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
