//! # 认证中间件
//!
//! 提供JWT Token验证和Claims提取功能。

use anyhow::Result;
use jsonwebtoken::{decode, DecodingKey, Validation, Algorithm};
use serde::{Deserialize, Serialize};
use std::env;
use tonic::metadata::MetadataMap;
use tracing::debug;

/// Token Claims
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenClaims {
    /// 用户ID
    pub user_id: String,
    /// 租户ID
    pub tenant_id: String,
    /// 角色列表
    pub roles: Vec<String>,
    /// 权限列表
    pub permissions: Vec<String>,
    /// 过期时间（Unix时间戳）
    pub exp: i64,
}

/// 认证中间件
pub struct AuthMiddleware {
    /// JWT密钥
    secret_key: Vec<u8>,
    /// 验证配置
    validation: Validation,
}

impl AuthMiddleware {
    /// 创建认证中间件
    pub fn new(secret_key: Vec<u8>) -> Self {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_exp = true;
        
        Self {
            secret_key,
            validation,
        }
    }
    
    /// 从环境变量创建认证中间件
    pub fn from_env() -> Result<Self> {
        let secret_key = env::var("JWT_SECRET_KEY")
            .unwrap_or_else(|_| "default-secret-key-change-in-production".to_string())
            .into_bytes();
        
        Ok(Self::new(secret_key))
    }
    
    /// 从Metadata中提取并验证Token
    pub fn authenticate(&self, metadata: &MetadataMap) -> Result<TokenClaims> {
        // 从Authorization header提取Token
        let token = metadata
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| {
                if s.starts_with("Bearer ") {
                    Some(s[7..].to_string())
                } else {
                    None
                }
            })
            .ok_or_else(|| anyhow::anyhow!("Missing or invalid Authorization header"))?;
        
        // 解码和验证Token
        let decoding_key = DecodingKey::from_secret(&self.secret_key);
        let token_data = decode::<TokenClaims>(&token, &decoding_key, &self.validation)
            .map_err(|e| anyhow::anyhow!("Token validation failed: {}", e))?;
        
        let claims = token_data.claims;
        
        debug!(
            user_id = %claims.user_id,
            tenant_id = %claims.tenant_id,
            "Token authenticated"
        );
        
        Ok(claims)
    }
}
