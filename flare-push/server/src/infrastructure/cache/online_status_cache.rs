//! 在线状态本地缓存（5秒TTL）
//!
//! 设计原则：
//! - 使用内存缓存减少对 Signaling Online 服务的调用
//! - 5秒TTL平衡数据新鲜度和性能
//! - 支持批量查询缓存

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use flare_server_core::error::Result;
use tokio::sync::RwLock;
use tracing::{debug, trace};

use crate::domain::repository::{OnlineStatus, OnlineStatusRepository};

/// 缓存条目
#[derive(Debug, Clone)]
struct CacheEntry {
    status: OnlineStatus,
    cached_at: Instant,
}

impl CacheEntry {
    fn is_expired(&self, ttl: Duration) -> bool {
        self.cached_at.elapsed() > ttl
    }
}

/// 带缓存的在线状态仓储实现
pub struct CachedOnlineStatusRepository {
    inner: Arc<dyn OnlineStatusRepository>,
    cache: Arc<RwLock<HashMap<String, CacheEntry>>>,
    ttl: Duration,
}

impl CachedOnlineStatusRepository {
    pub fn new(inner: Arc<dyn OnlineStatusRepository>, ttl_seconds: u64) -> Self {
        Self {
            inner,
            cache: Arc::new(RwLock::new(HashMap::new())),
            ttl: Duration::from_secs(ttl_seconds),
        }
    }

    /// 清理过期缓存
    async fn cleanup_expired(&self) {
        let mut cache = self.cache.write().await;
        let before = cache.len();
        cache.retain(|_, entry| !entry.is_expired(self.ttl));
        let after = cache.len();
        if before > after {
            debug!(
                removed = before - after,
                remaining = after,
                "Cleaned up expired online status cache entries"
            );
        }
    }
}

#[async_trait]
impl OnlineStatusRepository for CachedOnlineStatusRepository {
    async fn is_online(&self, user_id: &str) -> Result<bool> {
        let statuses = self.batch_get_online_status(&[user_id.to_string()]).await?;
        Ok(statuses.get(user_id).map(|s| s.online).unwrap_or(false))
    }

    async fn batch_get_online_status(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, OnlineStatus>> {
        // 1. 从缓存读取（未过期的）
        let mut result = HashMap::new();
        let mut missing_user_ids = Vec::new();

        {
            let cache = self.cache.read().await;
            for user_id in user_ids {
                if let Some(entry) = cache.get(user_id) {
                    if !entry.is_expired(self.ttl) {
                        result.insert(user_id.clone(), entry.status.clone());
                        trace!(user_id = %user_id, "Cache hit for online status");
                    } else {
                        missing_user_ids.push(user_id.clone());
                        trace!(user_id = %user_id, "Cache expired for online status");
                    }
                } else {
                    missing_user_ids.push(user_id.clone());
                    trace!(user_id = %user_id, "Cache miss for online status");
                }
            }
        }

        // 2. 查询缺失的用户（批量查询）
        if !missing_user_ids.is_empty() {
            debug!(
                missing_count = missing_user_ids.len(),
                cached_count = result.len(),
                "Querying online status for missing users"
            );

            let fetched = self
                .inner
                .batch_get_online_status(&missing_user_ids)
                .await?;

            // 3. 更新缓存
            {
                let mut cache = self.cache.write().await;
                for (user_id, status) in &fetched {
                    cache.insert(
                        user_id.clone(),
                        CacheEntry {
                            status: status.clone(),
                            cached_at: Instant::now(),
                        },
                    );
                }
            }

            // 4. 合并结果
            result.extend(fetched);
        }

        // 5. 定期清理过期缓存（每100次查询清理一次，避免频繁清理）
        if result.len() % 100 == 0 {
            self.cleanup_expired().await;
        }

        Ok(result)
    }

    async fn get_all_online_users_for_session(&self, conversation_id: &str) -> Result<Vec<String>> {
        // 直接委托给 inner（不缓存，因为 session 的在线用户列表变化频繁）
        self.inner
            .get_all_online_users_for_session(conversation_id)
            .await
    }
}
