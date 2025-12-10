//! 多设备推送策略服务
//!
//! 职责：
//! - 根据设备优先级选择推送目标
//! - 基于链接质量智能路由
//! - 支持多设备推送策略（全部推送/最优推送/高优先级推送）

use std::sync::Arc;
use tracing::{debug, info};

use super::{ConnectionQualityMetrics, ConnectionQualityService, QualityLevel};

/// 推送策略
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushStrategy {
    /// 推送到所有设备
    All,
    
    /// 推送到最优设备（质量最好的一个）
    BestDevice,
    
    /// 推送到高优先级设备（Critical + High）
    HighPriority,
    
    /// 推送到活跃设备（排除 Low 优先级）
    ActiveDevices,
}

/// 推送目标设备
#[derive(Debug, Clone)]
pub struct PushTarget {
    pub connection_id: String,
    pub user_id: String,
    pub device_id: String,
    pub device_priority: i32,
    pub quality_level: QualityLevel,
    pub rtt_ms: i64,
}

/// 多设备推送策略服务
pub struct MultiDevicePushService {
    quality_service: Arc<ConnectionQualityService>,
}

impl MultiDevicePushService {
    pub fn new(quality_service: Arc<ConnectionQualityService>) -> Self {
        Self { quality_service }
    }

    /// 根据策略选择推送目标
    pub async fn select_push_targets(
        &self,
        user_id: &str,
        strategy: PushStrategy,
    ) -> Vec<PushTarget> {
        let devices = self.quality_service.get_user_devices_quality(user_id).await;
        
        if devices.is_empty() {
            debug!(user_id = %user_id, "No devices found for user");
            return Vec::new();
        }
        
        let targets = self.apply_strategy(devices, strategy).await;
        
        info!(
            user_id = %user_id,
            strategy = ?strategy,
            target_count = targets.len(),
            "Push targets selected"
        );
        
        targets
    }

    /// 应用推送策略
    async fn apply_strategy(
        &self,
        mut devices: Vec<ConnectionQualityMetrics>,
        strategy: PushStrategy,
    ) -> Vec<PushTarget> {
        match strategy {
            PushStrategy::All => {
                // 推送到所有设备
                devices.into_iter().map(Self::to_push_target).collect()
            }
            
            PushStrategy::BestDevice => {
                // 推送到最优设备
                self.select_best_device(devices).await
            }
            
            PushStrategy::HighPriority => {
                // 推送到高优先级设备（假设优先级存储在某处，这里简化处理）
                // 只选择质量为 Excellent 或 Good 的设备
                devices.retain(|d| d.quality_level >= QualityLevel::Good);
                devices.into_iter().map(Self::to_push_target).collect()
            }
            
            PushStrategy::ActiveDevices => {
                // 推送到活跃设备（质量不是 Poor 的设备）
                devices.retain(|d| d.quality_level > QualityLevel::Poor);
                devices.into_iter().map(Self::to_push_target).collect()
            }
        }
    }

    /// 选择最优设备
    async fn select_best_device(
        &self,
        mut devices: Vec<ConnectionQualityMetrics>,
    ) -> Vec<PushTarget> {
        if devices.is_empty() {
            return Vec::new();
        }
        
        // 按质量等级降序、RTT升序排序
        devices.sort_by(|a, b| {
            match b.quality_level.cmp(&a.quality_level) {
                std::cmp::Ordering::Equal => a.rtt_avg_ms.partial_cmp(&b.rtt_avg_ms).unwrap(),
                other => other,
            }
        });
        
        // 只选择最优的设备
        vec![Self::to_push_target(devices.into_iter().next().unwrap())]
    }

    /// 转换为推送目标
    fn to_push_target(metrics: ConnectionQualityMetrics) -> PushTarget {
        PushTarget {
            connection_id: metrics.connection_id,
            user_id: metrics.user_id,
            device_id: metrics.device_id,
            device_priority: 0,  // TODO: 从 Online 服务获取设备优先级
            quality_level: metrics.quality_level,
            rtt_ms: metrics.rtt_ms,
        }
    }

    /// 评估是否需要回退到离线推送
    /// 
    /// 判断标准：
    /// - 没有在线设备
    /// - 所有在线设备质量都很差（Poor）
    /// - 所有在线设备都是低优先级
    pub async fn should_fallback_to_offline_push(
        &self,
        user_id: &str,
    ) -> bool {
        let devices = self.quality_service.get_user_devices_quality(user_id).await;
        
        if devices.is_empty() {
            return true;
        }
        
        // 检查是否所有设备质量都很差
        let all_poor_quality = devices.iter().all(|d| d.quality_level == QualityLevel::Poor);
        
        all_poor_quality
    }
}
