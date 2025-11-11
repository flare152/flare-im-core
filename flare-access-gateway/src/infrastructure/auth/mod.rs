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
use tracing::{debug, info, warn};

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
    fn verify_token(&self, token: &str) -> Option<String> {
        match self.token_service.validate_token(token) {
            Ok(claims) => Some(claims.sub),
            Err(err) => {
                warn!(?err, "Token validation failed");
                None
            }
        }
    }
}

#[async_trait]
impl Authenticator for TokenAuthenticator {
    async fn authenticate(
        &self,
        token: &str,
        connection_id: &str,
        device_info: Option<&DeviceInfo>,
        _metadata: Option<&HashMap<String, Vec<u8>>>,
    ) -> Result<AuthResult> {
        debug!(
            "[TokenAuthenticator] 验证 token: connection_id={}, token_len={}, device={:?}",
            connection_id,
            token.len(),
            device_info.map(|d| d.device_id.clone())
        );

        if let Some(user_id) = self.verify_token(token) {
            info!(
                "[TokenAuthenticator] ✅ Token 验证成功: connection_id={}, user_id={}",
                connection_id, user_id
            );

            Ok(AuthResult::success(Some(user_id)))
        } else {
            warn!(
                "[TokenAuthenticator] ❌ Token 验证失败: connection_id={}, token_preview={}",
                connection_id,
                if token.len() > 12 {
                    &token[..12]
                } else {
                    token
                }
            );

            Ok(AuthResult::failure("Token 无效或已过期".to_string()))
        }
    }
}
