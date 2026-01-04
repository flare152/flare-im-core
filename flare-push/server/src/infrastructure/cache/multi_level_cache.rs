//! 多级缓存实现
//!
//! 设计原则：
//! - L1: 本地缓存（5分钟 TTL，快速访问）
//! - L2: Redis 缓存（1小时 TTL，共享缓存）
//! - L3: 数据库/服务（持久化/实时查询）

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use flare_server_core::error::Result;
use tokio::sync::RwLock;
use tracing::{debug, trace};

use crate::domain::repository::{OnlineStatus, OnlineStatusRepository};

/// L1 缓存条目（本地内存）
#[derive(Debug, Clone)]
struct L1CacheEntry {
    status: OnlineStatus,
    cached_at: Instant,
}

impl L1CacheEntry {
    fn is_expired(&self, ttl: Duration) -> bool {
        self.cached_at.elapsed() > ttl
    }
}

/// 多级缓存实现
pub struct MultiLevelOnlineStatusCache {
    l1_cache: Arc<RwLock<HashMap<String, L1CacheEntry>>>,
    l2_repo: Option<Arc<dyn OnlineStatusRepository>>, // Redis 缓存（如果配置）
    l3_repo: Arc<dyn OnlineStatusRepository>,         // 底层服务（Signaling Online）
    l1_ttl: Duration,
    l2_ttl: Duration,
}

impl MultiLevelOnlineStatusCache {
    pub fn new(
        l3_repo: Arc<dyn OnlineStatusRepository>,
        l2_repo: Option<Arc<dyn OnlineStatusRepository>>,
        l1_ttl_seconds: u64,
        l2_ttl_seconds: u64,
    ) -> Self {
        Self {
            l1_cache: Arc::new(RwLock::new(HashMap::new())),
            l2_repo,
            l3_repo,
            l1_ttl: Duration::from_secs(l1_ttl_seconds),
            l2_ttl: Duration::from_secs(l2_ttl_seconds),
        }
    }

    /// 清理过期的 L1 缓存
    async fn cleanup_l1_expired(&self) {
        let mut cache = self.l1_cache.write().await;
        let before = cache.len();
        cache.retain(|_, entry| !entry.is_expired(self.l1_ttl));
        let after = cache.len();
        if before > after {
            debug!(
                removed = before - after,
                remaining = after,
                "Cleaned up expired L1 cache entries"
            );
        }
    }
}

#[async_trait]
impl OnlineStatusRepository for MultiLevelOnlineStatusCache {
    async fn is_online(&self, ctx: &flare_server_core::context::Context) -> Result<bool> {
        let user_id = ctx.user_id().ok_or_else(|| {
            flare_server_core::error::ErrorBuilder::new(
                flare_server_core::error::ErrorCode::InvalidParameter,
                "user_id is required in context"
            )
            .build_error()
        })?;
        let statuses = self.batch_get_online_status(&[user_id.to_string()]).await?;
        Ok(statuses.get(user_id).map(|s| s.online).unwrap_or(false))
    }

    async fn batch_get_online_status(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, OnlineStatus>> {
        let mut result = HashMap::new();
        let mut missing_user_ids = Vec::new();

        // 1. 查询 L1 缓存（本地内存）
        {
            let l1 = self.l1_cache.read().await;
            for user_id in user_ids {
                if let Some(entry) = l1.get(user_id) {
                    if !entry.is_expired(self.l1_ttl) {
                        result.insert(user_id.clone(), entry.status.clone());
                        trace!(user_id = %user_id, "L1 cache hit");
                    } else {
                        missing_user_ids.push(user_id.clone());
                    }
                } else {
                    missing_user_ids.push(user_id.clone());
                }
            }
        }

        // 2. 查询 L2 缓存（Redis，如果有配置）
        if !missing_user_ids.is_empty() {
            let mut l2_missing = Vec::new();

            if let Some(l2_repo) = &self.l2_repo {
                match l2_repo.batch_get_online_status(&missing_user_ids).await {
                    Ok(l2_result) => {
                        // 回填 L1 缓存
                        {
                            let mut l1 = self.l1_cache.write().await;
                            for (user_id, status) in &l2_result {
                                l1.insert(
                                    user_id.clone(),
                                    L1CacheEntry {
                                        status: status.clone(),
                                        cached_at: Instant::now(),
                                    },
                                );
                                result.insert(user_id.clone(), status.clone());
                            }
                        }

                        // 找出 L2 缓存中也没有的用户
                        for user_id in &missing_user_ids {
                            if !l2_result.contains_key(user_id) {
                                l2_missing.push(user_id.clone());
                            }
                        }
                    }
                    Err(e) => {
                        debug!(error = %e, "L2 cache query failed, falling back to L3");
                        l2_missing = missing_user_ids;
                    }
                }
            } else {
                l2_missing = missing_user_ids;
            }

            // 3. 查询 L3（底层服务）
            if !l2_missing.is_empty() {
                let l3_result = self.l3_repo.batch_get_online_status(&l2_missing).await?;

                // 回填 L2 缓存（如果有）
                if let Some(l2_repo) = &self.l2_repo {
                    // 注意：这里需要 L2 支持批量写入，如果没有则跳过
                    // 为了简化，我们暂时跳过 L2 回填，只回填 L1
                }

                // 回填 L1 缓存
                {
                    let mut l1 = self.l1_cache.write().await;
                    for (user_id, status) in &l3_result {
                        l1.insert(
                            user_id.clone(),
                            L1CacheEntry {
                                status: status.clone(),
                                cached_at: Instant::now(),
                            },
                        );
                        result.insert(user_id.clone(), status.clone());
                    }
                }
            }
        }

        // 4. 定期清理过期缓存
        if result.len() % 100 == 0 {
            self.cleanup_l1_expired().await;
        }

        Ok(result)
    }

    async fn get_all_online_users_for_session(&self, conversation_id: &str) -> Result<Vec<String>> {
        // 直接委托给 L3（不缓存，因为变化频繁）
        self.l3_repo
            .get_all_online_users_for_session(conversation_id)
            .await
    }
}
