//! 负载均衡器值对象
//!
//! 负责服务实例的负载均衡选择

use std::collections::HashMap;
use std::sync::Arc;

/// 负载均衡策略
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LoadBalancingStrategy {
    /// 轮询（默认）
    RoundRobin,
    /// 最小连接（需外部提供连接数指标）
    LeastConnections,
    /// 延迟感知（需外部提供P99延迟指标）
    LatencyAware,
}

/// 负载均衡器
///
///
/// 参考飞书Lark-Dispatcher设计：
/// - 轮询（Round Robin）：默认策略
/// - 最小连接（Least Connections）：动态负载感知
/// - 延迟感知（Latency-Aware）：P99延迟择优
#[derive(Clone)]
pub struct ServiceLoadBalancer {
    /// 负载均衡策略
    strategy: LoadBalancingStrategy,
    /// 轮询计数器（用于RoundRobin）
    robin_counter: Arc<std::sync::atomic::AtomicUsize>,
    /// 实例指标缓存（用于LeastConnections和LatencyAware策略）
    metrics_cache: Arc<std::sync::Mutex<HashMap<String, HashMap<String, u64>>>>,
}

impl ServiceLoadBalancer {
    pub fn new() -> Self {
        Self {
            strategy: LoadBalancingStrategy::RoundRobin,
            robin_counter: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            metrics_cache: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    pub fn with_strategy(strategy: LoadBalancingStrategy) -> Self {
        Self {
            strategy,
            robin_counter: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            metrics_cache: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    /// 获取实例指标（从缓存中获取，如果不存在则返回默认值）
    fn get_instance_metric(&self, instance_id: &str, metric_name: &str) -> u64 {
        let cache = self.metrics_cache.lock().unwrap();
        if let Some(instance_metrics) = cache.get(instance_id) {
            *instance_metrics.get(metric_name).unwrap_or(&0u64)
        } else {
            0
        }
    }

    /// 更新实例指标缓存
    pub fn update_instance_metrics(&self, instance_id: String, metrics: HashMap<String, u64>) {
        let mut cache = self.metrics_cache.lock().unwrap();
        cache.insert(instance_id, metrics);
    }

    /// 从字符串列表选择服务（简化版，用于向后兼容）
    ///
    /// **注意**: 此接口无法感知分片和机房，仅用于简单场景或测试
    pub fn pick_service(
        &self,
        _svid: &str,
        _shard: usize,
        candidates: &[String],
    ) -> Option<String> {
        if candidates.is_empty() {
            return None;
        }

        match self.strategy {
            LoadBalancingStrategy::RoundRobin => {
                let index = self
                    .robin_counter
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Some(candidates[index % candidates.len()].clone())
            }
            LoadBalancingStrategy::LeastConnections => {
                // 生产实现：查询指标缓存获取各服务实例的当前连接数
                let selected = candidates.iter().min_by_key(|endpoint| {
                    // 从endpoint中提取instance_id（假设格式为http://host:port）
                    let instance_id = endpoint.replace("http://", "").replace("https://", "");
                    self.get_instance_metric(&instance_id, "active_connections")
                });
                selected.cloned()
            }
            LoadBalancingStrategy::LatencyAware => {
                // 生产实现：查询指标缓存获取各服务实例的P99延迟
                let selected = candidates.iter().min_by_key(|endpoint| {
                    let instance_id = endpoint.replace("http://", "").replace("https://", "");
                    self.get_instance_metric(&instance_id, "p99_latency")
                });
                selected.cloned()
            }
        }
    }
}

