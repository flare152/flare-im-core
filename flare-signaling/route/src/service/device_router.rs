//! 设备级别路由服务
//!
//! 职责：
//! - 基于设备优先级的路由选择
//! - 基于链接质量的智能路由
//! - 支持设备级别的负载均衡

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// 设备路由信息
#[derive(Debug, Clone)]
pub struct DeviceRouteInfo {
    pub user_id: String,
    pub device_id: String,
    pub gateway_id: String,
    pub session_id: String,
    
    // 设备优先级（0=未指定, 1=低, 2=普通, 3=高, 4=关键）
    pub device_priority: i32,
    
    // 链接质量指标
    pub rtt_ms: i64,
    pub packet_loss_rate: f64,
    pub quality_score: f64,  // 综合评分 0-100
    
    // 最后更新时间
    pub last_update_ts: i64,
}

impl DeviceRouteInfo {
    /// 计算质量评分（0-100）
    /// 
    /// 评分规则：
    /// - RTT < 50ms: 基础分 90-100
    /// - RTT 50-100ms: 基础分 70-90
    /// - RTT 100-200ms: 基础分 40-70
    /// - RTT > 200ms: 基础分 0-40
    /// - 丢包率每 1% 扣 10 分
    pub fn calculate_quality_score(rtt_ms: i64, packet_loss_rate: f64) -> f64 {
        let rtt_score = if rtt_ms < 50 {
            100.0 - (rtt_ms as f64 / 5.0)  // 90-100
        } else if rtt_ms < 100 {
            90.0 - ((rtt_ms - 50) as f64 / 2.5)  // 70-90
        } else if rtt_ms < 200 {
            70.0 - ((rtt_ms - 100) as f64 / 3.33)  // 40-70
        } else {
            40.0 - ((rtt_ms - 200).min(200) as f64 / 5.0)  // 0-40
        };
        
        let loss_penalty = packet_loss_rate * 1000.0;  // 每 1% 扣 10 分
        
        (rtt_score - loss_penalty).max(0.0).min(100.0)
    }
}

/// 设备级别路由服务
pub struct DeviceRouter {
    // user_id -> Vec<DeviceRouteInfo>
    device_routes: Arc<RwLock<HashMap<String, Vec<DeviceRouteInfo>>>>,
}

impl DeviceRouter {
    pub fn new() -> Self {
        Self {
            device_routes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 注册或更新设备路由信息
    pub async fn register_device(
        &self,
        user_id: String,
        device_id: String,
        gateway_id: String,
        session_id: String,
        device_priority: i32,
        rtt_ms: i64,
        packet_loss_rate: f64,
    ) {
        let quality_score = DeviceRouteInfo::calculate_quality_score(rtt_ms, packet_loss_rate);
        
        let device_info = DeviceRouteInfo {
            user_id: user_id.clone(),
            device_id: device_id.clone(),
            gateway_id,
            session_id,
            device_priority,
            rtt_ms,
            packet_loss_rate,
            quality_score,
            last_update_ts: chrono::Utc::now().timestamp_millis(),
        };
        
        let mut routes = self.device_routes.write().await;
        let user_devices = routes.entry(user_id.clone()).or_insert_with(Vec::new);
        
        // 更新或添加设备
        if let Some(existing) = user_devices.iter_mut().find(|d| d.device_id == device_id) {
            *existing = device_info;
        } else {
            user_devices.push(device_info);
        }
        
        debug!(
            user_id = %user_id,
            device_id = %device_id,
            quality_score = quality_score,
            "Device route registered"
        );
    }

    /// 移除设备路由
    pub async fn unregister_device(&self, user_id: &str, device_id: &str) {
        let mut routes = self.device_routes.write().await;
        if let Some(user_devices) = routes.get_mut(user_id) {
            user_devices.retain(|d| d.device_id != device_id);
            if user_devices.is_empty() {
                routes.remove(user_id);
            }
        }
    }

    /// 选择最优设备（基于优先级和链接质量）
    /// 
    /// 选择策略：
    /// 1. 首先按设备优先级排序（Critical > High > Normal > Low）
    /// 2. 同优先级内按质量评分排序
    /// 3. 返回评分最高的设备
    pub async fn select_best_device(&self, user_id: &str) -> Option<DeviceRouteInfo> {
        let routes = self.device_routes.read().await;
        let user_devices = routes.get(user_id)?;
        
        if user_devices.is_empty() {
            return None;
        }
        
        let mut sorted_devices = user_devices.clone();
        sorted_devices.sort_by(|a, b| {
            // 首先按优先级降序
            match b.device_priority.cmp(&a.device_priority) {
                std::cmp::Ordering::Equal => {
                    // 同优先级按质量评分降序
                    b.quality_score.partial_cmp(&a.quality_score).unwrap()
                }
                other => other,
            }
        });
        
        let best = sorted_devices.into_iter().next()?;
        
        info!(
            user_id = %user_id,
            device_id = %best.device_id,
            gateway_id = %best.gateway_id,
            priority = best.device_priority,
            quality_score = best.quality_score,
            "Best device selected"
        );
        
        Some(best)
    }

    /// 选择高优先级设备列表
    /// 
    /// 返回所有 High (3) 和 Critical (4) 优先级的设备
    pub async fn select_high_priority_devices(&self, user_id: &str) -> Vec<DeviceRouteInfo> {
        let routes = self.device_routes.read().await;
        let user_devices = match routes.get(user_id) {
            Some(devices) => devices,
            None => return Vec::new(),
        };
        
        user_devices
            .iter()
            .filter(|d| d.device_priority >= 3)  // High 或 Critical
            .cloned()
            .collect()
    }

    /// 选择所有活跃设备（排除 Low 优先级和低质量设备）
    pub async fn select_active_devices(&self, user_id: &str) -> Vec<DeviceRouteInfo> {
        let routes = self.device_routes.read().await;
        let user_devices = match routes.get(user_id) {
            Some(devices) => devices,
            None => return Vec::new(),
        };
        
        user_devices
            .iter()
            .filter(|d| {
                // 排除 Low 优先级（1）和质量评分低于 40 的设备
                d.device_priority >= 2 && d.quality_score >= 40.0
            })
            .cloned()
            .collect()
    }

    /// 获取用户所有设备路由信息
    pub async fn get_user_devices(&self, user_id: &str) -> Vec<DeviceRouteInfo> {
        let routes = self.device_routes.read().await;
        routes.get(user_id).cloned().unwrap_or_default()
    }

    /// 清理过期设备（超过5分钟未更新）
    pub async fn cleanup_expired(&self, expiration_ms: i64) {
        let mut routes = self.device_routes.write().await;
        let now = chrono::Utc::now().timestamp_millis();
        
        for (_user_id, devices) in routes.iter_mut() {
            devices.retain(|d| now - d.last_update_ts < expiration_ms);
        }
        
        // 移除没有设备的用户
        routes.retain(|_, devices| !devices.is_empty());
    }
}

impl Default for DeviceRouter {
    fn default() -> Self {
        Self::new()
    }
}
