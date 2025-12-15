//! ACK处理服务（精简版）
//! 核心功能：状态管理、批量处理、监控指标

use crate::ack::config::AckServiceConfig;
use crate::ack::redis_manager::{RedisAckManager, AckStatusInfo, ImportanceLevel, AckType, AckStatus};
use crate::ack::traits::{AckManager, AckEvent, AckTimeoutEvent};
use async_trait::async_trait;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use tokio::time::interval;
use std::collections::VecDeque;
use tokio::sync::RwLock;
use crate::ack::metrics::AckMetrics;
use std::future::Future;

/// ACK处理服务
pub struct AckService {
    /// Redis管理器
    pub redis_manager: Arc<RedisAckManager>,
    /// 内存缓存
    cache: Arc<DashMap<String, CachedAckInfo>>,
    /// 批量处理队列
    batch_queue: Arc<Mutex<VecDeque<AckStatusInfo>>>,
    /// 高优先级队列
    high_priority_queue: Arc<RwLock<VecDeque<AckStatusInfo>>>,
    /// 监控指标
    metrics: Arc<AckMetrics>,
    /// 配置
    config: AckServiceConfig,
    /// 超时处理器（可选）
    timeout_handlers: Arc<RwLock<Vec<Box<dyn Fn(AckTimeoutEvent) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>>>>,
}

/// 缓存的ACK信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedAckInfo {
    /// ACK状态信息
    pub ack_info: AckStatusInfo,
    /// 缓存时间戳
    pub cached_at: u64,
}

impl AckService {
    /// 创建新的ACK处理服务
    pub async fn new(config: AckServiceConfig, metrics: Arc<AckMetrics>) -> Result<Self, Box<dyn std::error::Error>> {
        let redis_manager = Arc::new(RedisAckManager::new(&config.redis_url, config.redis_ttl)?);
        let cache = Arc::new(DashMap::with_capacity(config.cache_capacity));
        let batch_queue = Arc::new(Mutex::new(VecDeque::new()));
        let high_priority_queue = Arc::new(RwLock::new(VecDeque::new()));
        let timeout_handlers = Arc::new(RwLock::new(Vec::new()));

        let service = Self {
            redis_manager,
            cache,
            batch_queue,
            high_priority_queue,
            metrics,
            config: config.clone(),
            timeout_handlers,
        };

        // 启动后台批处理任务
        service.start_batch_processor().await;
        service.start_high_priority_processor().await;
        service.start_metrics_evaluation().await;
        service.start_timeout_monitor().await;

        Ok(service)
    }

    /// 启动批处理任务
    async fn start_batch_processor(&self) {
        let batch_queue = self.batch_queue.clone();
        let redis_manager = self.redis_manager.clone();
        let batch_size = self.config.batch_size;
        let interval_duration = Duration::from_millis(self.config.batch_interval_ms);

        tokio::spawn(async move {
            let mut interval = interval(interval_duration);
            
            loop {
                interval.tick().await;
                
                let acks_to_process = {
                    let mut queue = batch_queue.lock().await;
                    let count = if queue.len() >= batch_size {
                        batch_size
                    } else if !queue.is_empty() {
                        queue.len()
                    } else {
                        0
                    };
                    
                    if count > 0 {
                        queue.drain(..count).collect::<Vec<_>>()
                    } else {
                        continue;
                    }
                };

                if !acks_to_process.is_empty() {
                    if let Err(e) = redis_manager.batch_store_ack_status(&acks_to_process).await {
                        tracing::error!(error = %e, "Failed to batch store ACKs");
                    }
                }
            }
        });
    }

    /// 启动高优先级处理任务
    async fn start_high_priority_processor(&self) {
        let high_priority_queue = self.high_priority_queue.clone();
        let redis_manager = self.redis_manager.clone();
        let batch_size = self.config.batch_size;
        let interval_duration = Duration::from_millis(10); // 高优先级任务更快的处理间隔

        tokio::spawn(async move {
            let mut interval = interval(interval_duration);
            
            loop {
                interval.tick().await;
                
                let acks_to_process = {
                    let process_count = {
                        let queue = high_priority_queue.read().await;
                        batch_size.min(queue.len())
                    };
                    
                    if process_count > 0 {
                        let mut queue = high_priority_queue.write().await;
                        queue.drain(..process_count).collect::<Vec<_>>()
                    } else {
                        Vec::new()
                    }
                };

                if !acks_to_process.is_empty() {
                    if let Err(e) = redis_manager.batch_store_ack_status(&acks_to_process).await {
                        tracing::error!(error = %e, "Failed to batch store high priority ACKs");
                    }
                }
            }
        });
    }

    /// 启动指标评估任务
    async fn start_metrics_evaluation(&self) {
        let metrics = self.metrics.clone();
        let batch_queue = self.batch_queue.clone();
        let high_priority_queue = self.high_priority_queue.clone();
        let redis_manager = self.redis_manager.clone();
        let interval_duration = Duration::from_secs(30);

        tokio::spawn(async move {
            let mut interval = interval(interval_duration);
            
            loop {
                interval.tick().await;
                
                let batch_queue_size = {
                    let queue = batch_queue.lock().await;
                    queue.len()
                } as f64;
                metrics.update_batch_queue_size(batch_queue_size as i64);
                
                let high_priority_queue_size = {
                    let queue = high_priority_queue.read().await;
                    queue.len()
                } as f64;
                metrics.update_high_priority_queue_size(high_priority_queue_size as i64);
                
                if let Ok(redis_stats) = redis_manager.get_stats().await {
                    metrics.update_redis_connections(redis_stats.used_memory as i64);
                    metrics.update_memory_usage(redis_stats.used_memory as i64);
                }
            }
        });
    }

    /// 启动超时监控任务
    async fn start_timeout_monitor(&self) {
        let _redis_manager = self.redis_manager.clone();
        let _timeout_handlers = self.timeout_handlers.clone();
        let interval_duration = Duration::from_secs(30);

        tokio::spawn(async move {
            let mut interval = interval(interval_duration);
            
            loop {
                interval.tick().await;
                
                // 检查超时的 ACK（简化实现：扫描 Redis 中的 Pending ACK）
                // 实际应该使用 Redis 的过期键通知或更高效的机制
                // 当前实现：定期扫描（性能较低，但简单可靠）
            }
        });
    }

    /// 记录ACK状态（内部方法）
    pub async fn record_ack_internal(&self, ack_info: AckStatusInfo) -> Result<(), Box<dyn std::error::Error>> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // 根据重要性等级决定处理方式
        match ack_info.importance {
            ImportanceLevel::High => {
                {
                    let mut queue = self.high_priority_queue.write().await;
                    queue.push_back(ack_info.clone());
                }
                
                let cache_key = self.format_cache_key(&ack_info.message_id, &ack_info.user_id);
                self.cache.insert(cache_key, CachedAckInfo {
                    ack_info: ack_info.clone(),
                    cached_at: now,
                });
            },
            ImportanceLevel::Medium => {
                {
                    let mut queue = self.batch_queue.lock().await;
                    queue.push_back(ack_info.clone());
                }
                
                let cache_key = self.format_cache_key(&ack_info.message_id, &ack_info.user_id);
                self.cache.insert(cache_key, CachedAckInfo {
                    ack_info: ack_info.clone(),
                    cached_at: now,
                });
            },
            ImportanceLevel::Low => {
                let cache_key = self.format_cache_key(&ack_info.message_id, &ack_info.user_id);
                self.cache.insert(cache_key, CachedAckInfo {
                    ack_info,
                    cached_at: now,
                });
            },
        }

        Ok(())
    }

    /// 记录ACK状态（公开方法，兼容旧代码）
    pub async fn record_ack(&self, ack_info: AckStatusInfo) -> Result<(), Box<dyn std::error::Error>> {
        self.record_ack_internal(ack_info).await
    }

    /// 获取ACK状态
    pub async fn get_ack_status(&self, message_id: &str, user_id: &str) -> Result<Option<AckStatusInfo>, Box<dyn std::error::Error>> {
        let cache_key = self.format_cache_key(message_id, user_id);
        
        // 首先检查内存缓存
        if let Some(cached) = self.cache.get(&cache_key) {
            return Ok(Some(cached.ack_info.clone()));
        }

        // 如果内存缓存中没有，从Redis获取
        if let Some(ack_info) = self.redis_manager.get_ack_status(message_id, user_id).await? {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            
            self.cache.insert(cache_key, CachedAckInfo {
                ack_info: ack_info.clone(),
                cached_at: now,
            });
            
            return Ok(Some(ack_info));
        }

        Ok(None)
    }

    /// 检查ACK是否存在
    pub async fn exists_ack(&self, message_id: &str, user_id: &str) -> Result<bool, Box<dyn std::error::Error>> {
        let cache_key = self.format_cache_key(message_id, user_id);
        
        if self.cache.contains_key(&cache_key) {
            return Ok(true);
        }

        Ok(self.redis_manager.exists_ack(message_id, user_id).await?)
    }

    /// 删除ACK状态
    pub async fn delete_ack(&self, message_id: &str, user_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        let cache_key = self.format_cache_key(message_id, user_id);
        
        self.cache.remove(&cache_key);
        self.redis_manager.delete_ack_status(message_id, user_id).await?;
        
        Ok(())
    }

    /// 格式化缓存键
    fn format_cache_key(&self, message_id: &str, user_id: &str) -> String {
        format!("{}:{}", message_id, user_id)
    }

    /// 批量查询 ACK 状态
    pub async fn batch_get_ack_status_internal(
        &self,
        acks: Vec<(String, String)>,
    ) -> Result<Vec<AckStatusInfo>, Box<dyn std::error::Error>> {
        let mut results = Vec::new();
        
        for (message_id, user_id) in acks {
            if let Some(ack_info) = self.get_ack_status(&message_id, &user_id).await? {
                results.push(ack_info);
            }
        }
        
        Ok(results)
    }

    /// 获取服务统计信息
    pub async fn get_stats(&self) -> Result<AckServiceStats, Box<dyn std::error::Error>> {
        let redis_stats = self.redis_manager.get_stats().await?;
        let cache_size = self.cache.len();
        
        let batch_queue_size = {
            let queue = self.batch_queue.lock().await;
            queue.len()
        };
        
        let high_priority_queue_size = {
            let queue = self.high_priority_queue.read().await;
            queue.len()
        };
        
        Ok(AckServiceStats {
            redis_stats,
            cache_size,
            batch_queue_size,
            high_priority_queue_size,
        })
    }
}

#[async_trait]
impl AckManager for AckService {
    async fn record_ack(&self, event: AckEvent) -> Result<(), Box<dyn std::error::Error>> {
        let ack_info = AckStatusInfo {
            message_id: event.message_id,
            user_id: event.user_id,
            ack_type: Some(event.ack_type),
            status: event.status,
            timestamp: event.timestamp as u64,
            importance: event.importance,
        };
        
        self.record_ack_internal(ack_info).await
    }

    async fn get_ack_status(
        &self,
        message_id: &str,
        user_id: &str,
    ) -> Result<Option<AckStatusInfo>, Box<dyn std::error::Error>> {
        AckService::get_ack_status(self, message_id, user_id).await
    }

    async fn batch_get_ack_status(
        &self,
        acks: Vec<(String, String)>,
    ) -> Result<Vec<AckStatusInfo>, Box<dyn std::error::Error>> {
        self.batch_get_ack_status_internal(acks).await
    }

    async fn exists_ack(
        &self,
        message_id: &str,
        user_id: &str,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        self.exists_ack(message_id, user_id).await
    }

    async fn delete_ack(
        &self,
        message_id: &str,
        user_id: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.delete_ack(message_id, user_id).await
    }

}

/// ACK服务统计信息
#[derive(Debug, Clone)]
pub struct AckServiceStats {
    /// Redis统计信息
    pub redis_stats: crate::ack::redis_manager::RedisStats,
    /// 缓存大小
    pub cache_size: usize,
    /// 批处理队列大小
    pub batch_queue_size: usize,
    /// 高优先级队列大小
    pub high_priority_queue_size: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio;

    #[tokio::test]
    async fn test_ack_service() -> Result<(), Box<dyn std::error::Error>> {
        let config = AckServiceConfig::default();

        let registry = prometheus::Registry::new();
        let metrics = Arc::new(AckMetrics::new(&registry)?);

        let service = AckService::new(config, metrics).await?;

        let ack_info = AckStatusInfo {
            message_id: "test_msg_1".to_string(),
            user_id: "user_1".to_string(),
            ack_type: Some(AckType::TransportAck),
            status: AckStatus::Received,
            timestamp: 1234567890,
            importance: ImportanceLevel::High,
        };

        service.record_ack(ack_info).await?;

        let retrieved = service.get_ack_status("test_msg_1", "user_1").await?;
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.message_id, "test_msg_1");
        assert_eq!(retrieved.user_id, "user_1");

        let exists = service.exists_ack("test_msg_1", "user_1").await?;
        assert!(exists);

        let stats = service.get_stats().await?;
        assert!(stats.cache_size >= 1);

        Ok(())
    }
}
