//! # Hook查询服务
//!
//! 提供Hook配置和统计信息的查询接口

use std::sync::Arc;

use crate::domain::models::HookStatistics;
use crate::infrastructure::monitoring::MetricsCollector;

/// Hook查询服务
pub struct HookQueryService {
    metrics_collector: Arc<MetricsCollector>,
}

impl HookQueryService {
    pub fn new(metrics_collector: Arc<MetricsCollector>) -> Self {
        Self {
            metrics_collector,
        }
    }

    /// 获取Hook统计信息
    pub async fn get_statistics(&self, hook_name: &str) -> Option<HookStatistics> {
        self.metrics_collector.get_statistics(hook_name).await
    }

    /// 获取所有Hook统计信息
    pub async fn get_all_statistics(&self) -> std::collections::HashMap<String, HookStatistics> {
        self.metrics_collector.get_all_statistics().await
    }
}

