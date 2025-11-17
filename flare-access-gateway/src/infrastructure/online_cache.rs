//! 在线状态本地缓存
//!
//! 用于减少对Signaling Online的查询，提高推送性能

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;
use tracing::{debug, warn};

/// 在线状态缓存条目
#[derive(Clone, Debug)]
struct OnlineCacheEntry {
    user_id: String,
    gateway_id: String,
    online: bool,
    cached_at: Instant,
}

/// 在线状态本地缓存
pub struct OnlineStatusCache {
    cache: Arc<RwLock<HashMap<String, OnlineCacheEntry>>>,
    ttl: Duration,
    max_size: usize,
}

impl OnlineStatusCache {
    /// 创建新的在线状态缓存
    pub fn new(ttl_seconds: u64, max_size: usize) -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            ttl: Duration::from_secs(ttl_seconds),
            max_size,
        }
    }

    /// 获取用户在线状态
    /// 
    /// 返回: (online, gateway_id)
    pub async fn get(&self, user_id: &str) -> Option<(bool, String)> {
        let cache = self.cache.read().await;
        
        if let Some(entry) = cache.get(user_id) {
            // 检查是否过期
            if entry.cached_at.elapsed() < self.ttl {
                debug!(
                    user_id = %user_id,
                    gateway_id = %entry.gateway_id,
                    online = entry.online,
                    "Online status cache hit"
                );
                return Some((entry.online, entry.gateway_id.clone()));
            } else {
                debug!(
                    user_id = %user_id,
                    "Online status cache expired"
                );
            }
        }
        
        None
    }

    /// 设置用户在线状态
    pub async fn set(&self, user_id: String, gateway_id: String, online: bool) {
        let user_id_clone = user_id.clone();
        let gateway_id_clone = gateway_id.clone();
        let mut cache = self.cache.write().await;
        
        // 如果缓存已满，清理过期条目
        if cache.len() >= self.max_size {
            self.cleanup_expired(&mut cache).await;
        }
        
        // 如果仍然满，清理最旧的条目
        if cache.len() >= self.max_size {
            self.cleanup_oldest(&mut cache).await;
        }
        
        cache.insert(
            user_id_clone.clone(),
            OnlineCacheEntry {
                user_id: user_id_clone.clone(),
                gateway_id: gateway_id_clone.clone(),
                online,
                cached_at: Instant::now(),
            },
        );
        
        debug!(
            user_id = %user_id_clone,
            gateway_id = %gateway_id_clone,
            online = online,
            cache_size = cache.len(),
            "Online status cached"
        );
    }

    /// 批量设置用户在线状态
    pub async fn set_batch(&self, entries: Vec<(String, String, bool)>) {
        let mut cache = self.cache.write().await;
        
        // 如果缓存已满，清理过期条目
        if cache.len() >= self.max_size {
            self.cleanup_expired(&mut cache).await;
        }
        
        for (user_id, gateway_id, online) in entries {
            // 如果仍然满，清理最旧的条目
            if cache.len() >= self.max_size {
                self.cleanup_oldest(&mut cache).await;
            }
            
            cache.insert(
                user_id.clone(),
                OnlineCacheEntry {
                    user_id,
                    gateway_id: gateway_id.clone(),
                    online,
                    cached_at: Instant::now(),
                },
            );
        }
        
        debug!(
            cache_size = cache.len(),
            "Batch online status cached"
        );
    }

    /// 删除用户在线状态
    pub async fn remove(&self, user_id: &str) {
        let mut cache = self.cache.write().await;
        cache.remove(user_id);
        
        debug!(
            user_id = %user_id,
            "Online status removed from cache"
        );
    }

    /// 清理过期条目
    async fn cleanup_expired(&self, cache: &mut HashMap<String, OnlineCacheEntry>) {
        let now = Instant::now();
        let mut expired_keys = Vec::new();
        
        for (key, entry) in cache.iter() {
            if now.duration_since(entry.cached_at) >= self.ttl {
                expired_keys.push(key.clone());
            }
        }
        
        let cleaned_count = expired_keys.len();
        for key in expired_keys {
            cache.remove(&key);
        }
        
        if cleaned_count > 0 {
            debug!(
                cleaned_count = cleaned_count,
                remaining_size = cache.len(),
                "Cleaned up expired cache entries"
            );
        }
    }

    /// 清理最旧的条目（保留一半）
    async fn cleanup_oldest(&self, cache: &mut HashMap<String, OnlineCacheEntry>) {
        let target_size = self.max_size / 2;
        let current_size = cache.len();
        if current_size <= target_size {
            return;
        }
        
        let mut entries: Vec<_> = cache.iter().collect();
        entries.sort_by_key(|(_, entry)| entry.cached_at);
        
        let remove_count = current_size - target_size;
        let keys_to_remove: Vec<String> = entries.iter()
            .take(remove_count)
            .map(|(key, _)| (*key).clone())
            .collect();
        
        for key in keys_to_remove {
            cache.remove(&key);
        }
        
        warn!(
            removed_count = remove_count,
            remaining_size = cache.len(),
            "Cleaned up oldest cache entries due to size limit"
        );
    }

    /// 获取缓存统计信息
    pub async fn stats(&self) -> CacheStats {
        let cache = self.cache.read().await;
        let now = Instant::now();
        let mut expired_count = 0;
        
        for entry in cache.values() {
            if now.duration_since(entry.cached_at) >= self.ttl {
                expired_count += 1;
            }
        }
        
        CacheStats {
            total_entries: cache.len(),
            expired_entries: expired_count,
            max_size: self.max_size,
            ttl_seconds: self.ttl.as_secs(),
        }
    }
}

/// 缓存统计信息
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub total_entries: usize,
    pub expired_entries: usize,
    pub max_size: usize,
    pub ttl_seconds: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_cache_set_get() {
        let cache = OnlineStatusCache::new(30, 1000);
        
        cache.set("user1".to_string(), "gateway1".to_string(), true).await;
        
        let result = cache.get("user1").await;
        assert_eq!(result, Some((true, "gateway1".to_string())));
    }

    #[tokio::test]
    async fn test_cache_expiration() {
        let cache = OnlineStatusCache::new(1, 1000); // 1秒TTL
        
        cache.set("user1".to_string(), "gateway1".to_string(), true).await;
        
        // 立即查询应该命中
        assert!(cache.get("user1").await.is_some());
        
        // 等待过期
        sleep(Duration::from_secs(2)).await;
        
        // 应该过期
        assert!(cache.get("user1").await.is_none());
    }

    #[tokio::test]
    async fn test_cache_batch_set() {
        let cache = OnlineStatusCache::new(30, 1000);
        
        let entries = vec![
            ("user1".to_string(), "gateway1".to_string(), true),
            ("user2".to_string(), "gateway2".to_string(), false),
        ];
        
        cache.set_batch(entries).await;
        
        assert_eq!(cache.get("user1").await, Some((true, "gateway1".to_string())));
        assert_eq!(cache.get("user2").await, Some((false, "gateway2".to_string())));
    }
}

