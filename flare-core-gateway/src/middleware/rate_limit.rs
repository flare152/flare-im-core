//! # 限流中间件
//!
//! 提供基于令牌桶算法的限流功能，支持租户/IP/用户级别的限流。

use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::debug;

use crate::middleware::auth::TokenClaims;

/// 令牌桶
struct TokenBucket {
    /// 当前令牌数
    tokens: f64,
    /// 最大令牌数
    capacity: f64,
    /// 令牌填充速率（每秒）
    refill_rate: f64,
    /// 上次更新时间
    last_update: Instant,
}

impl TokenBucket {
    fn new(capacity: f64, refill_rate: f64) -> Self {
        Self {
            tokens: capacity,
            capacity,
            refill_rate,
            last_update: Instant::now(),
        }
    }
    
    /// 尝试消费令牌
    fn try_consume(&mut self, tokens: f64) -> bool {
        // 填充令牌
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_update).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.capacity);
        self.last_update = now;
        
        // 检查是否有足够的令牌
        if self.tokens >= tokens {
            self.tokens -= tokens;
            true
        } else {
            false
        }
    }
}

/// 限流中间件
#[derive(Clone)]
pub struct RateLimitMiddleware {
    /// 租户级别限流配置
    tenant_limit: Arc<RwLock<HashMap<String, TokenBucket>>>,
    /// IP级别限流配置
    ip_limit: Arc<RwLock<HashMap<String, TokenBucket>>>,
    /// 用户级别限流配置
    user_limit: Arc<RwLock<HashMap<String, TokenBucket>>>,
    /// 默认限流配置
    default_capacity: f64,
    default_refill_rate: f64,
}

impl Default for RateLimitMiddleware {
    fn default() -> Self {
        Self {
            tenant_limit: Arc::new(RwLock::new(HashMap::new())),
            ip_limit: Arc::new(RwLock::new(HashMap::new())),
            user_limit: Arc::new(RwLock::new(HashMap::new())),
            default_capacity: 100.0,
            default_refill_rate: 10.0,
        }
    }
}

impl RateLimitMiddleware {
    /// 创建限流中间件
    pub fn new(capacity: f64, refill_rate: f64) -> Self {
        Self {
            tenant_limit: Arc::new(RwLock::new(HashMap::new())),
            ip_limit: Arc::new(RwLock::new(HashMap::new())),
            user_limit: Arc::new(RwLock::new(HashMap::new())),
            default_capacity: capacity,
            default_refill_rate: refill_rate,
        }
    }
    
    /// 检查限流
    pub async fn check_rate_limit(
        &self,
        claims: &TokenClaims,
        client_ip: Option<&str>,
    ) -> Result<()> {
        // 1. 租户级别限流
        {
            let mut buckets = self.tenant_limit.write().await;
            let bucket = buckets
                .entry(claims.tenant_id.clone())
                .or_insert_with(|| {
                    TokenBucket::new(self.default_capacity, self.default_refill_rate)
                });
            
            if !bucket.try_consume(1.0) {
                debug!(
                    tenant_id = %claims.tenant_id,
                    "Tenant rate limit exceeded"
                );
                return Err(anyhow::anyhow!("Tenant rate limit exceeded"));
            }
        }
        
        // 2. 用户级别限流
        {
            let mut buckets = self.user_limit.write().await;
            let bucket = buckets
                .entry(claims.user_id.clone())
                .or_insert_with(|| {
                    TokenBucket::new(self.default_capacity, self.default_refill_rate)
                });
            
            if !bucket.try_consume(1.0) {
                debug!(
                    user_id = %claims.user_id,
                    "User rate limit exceeded"
                );
                return Err(anyhow::anyhow!("User rate limit exceeded"));
            }
        }
        
        // 3. IP级别限流（如果提供了IP）
        if let Some(ip) = client_ip {
            let mut buckets = self.ip_limit.write().await;
            let bucket = buckets
                .entry(ip.to_string())
                .or_insert_with(|| {
                    TokenBucket::new(self.default_capacity, self.default_refill_rate)
                });
            
            if !bucket.try_consume(1.0) {
                debug!(
                    ip = %ip,
                    "IP rate limit exceeded"
                );
                return Err(anyhow::anyhow!("IP rate limit exceeded"));
            }
        }
        
        Ok(())
    }
}
