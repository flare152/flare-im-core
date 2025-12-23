//! 设备路由处理器（查询侧）
//!
//! 负责设备路由查询的业务流程编排

use std::sync::Arc;
use anyhow::{Context, Result};
use tracing::info;

use crate::domain::entities::device_route::DeviceRoute;
use crate::infrastructure::OnlineServiceClient;
use flare_proto::signaling::router::PushStrategy;

/// 设备路由处理器
///
/// 职责：
/// - 编排设备路由查询流程
/// - 从 Online 服务查询设备信息
/// - 根据策略选择目标设备
pub struct DeviceRouteHandler {
    online_client: Arc<OnlineServiceClient>,
}

impl DeviceRouteHandler {
    pub fn new(online_client: Arc<OnlineServiceClient>) -> Self {
        Self { online_client }
    }

    /// 根据策略选择推送目标设备
    ///
    /// # 参数
    /// * `user_id` - 用户ID
    /// * `strategy` - 推送策略
    ///
    /// # 返回
    /// 选中的设备路由列表
    pub async fn select_push_targets(
        &self,
        user_id: &str,
        strategy: PushStrategy,
    ) -> Result<Vec<DeviceRoute>> {
        info!(user_id = %user_id, strategy = ?strategy, "Selecting push targets");

        // 从 Online 服务查询用户的所有在线设备
        let devices_resp = self
            .online_client
            .list_user_devices(user_id)
            .await
            .context("Failed to query user devices from Online service")?;

        // 转换为设备路由
        let mut routes: Vec<DeviceRoute> = devices_resp
            .devices
            .into_iter()
            .map(|d| DeviceRoute::new(
                user_id.to_string(),
                d.device_id,
                d.gateway_id,
                d.server_id,
                d.priority,
                calculate_quality_score(&d.connection_quality),
            ))
            .collect();

        // 根据策略选择
        let selected_routes = match strategy {
            PushStrategy::AllDevices => routes,
            PushStrategy::BestDevice => {
                // 选择单个最优设备（优先级最高 + 质量最好）
                let mut sorted_routes = routes;
                sorted_routes.sort_by(|a, b| {
                    b.device_priority.cmp(&a.device_priority).then_with(|| {
                        b.quality_score
                            .partial_cmp(&a.quality_score)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                });
                sorted_routes.into_iter().take(1).collect()
            }
            PushStrategy::ActiveDevices => {
                // 排除 Low 优先级设备 (priority = 0)
                routes.retain(|r| r.device_priority > 0);
                routes
            }
            PushStrategy::PrimaryDevice => {
                // 只选择优先级最高的设备
                if let Some(max_priority) = routes.iter().map(|r| r.device_priority).max() {
                    routes.retain(|r| r.device_priority == max_priority);
                }
                routes.into_iter().take(1).collect()
            }
            _ => routes, // 默认返回所有设备
        };

        info!(
            user_id = %user_id,
            strategy = ?strategy,
            selected_count = selected_routes.len(),
            "Push targets selected"
        );

        Ok(selected_routes)
    }

    /// 获取设备路由
    ///
    /// # 参数
    /// * `user_id` - 用户ID
    /// * `device_id` - 设备ID
    ///
    /// # 返回
    /// 设备路由，如果不存在则返回 None
    pub async fn get_device_route(
        &self,
        user_id: &str,
        device_id: &str,
    ) -> Result<Option<DeviceRoute>> {
        info!(user_id = %user_id, device_id = %device_id, "Getting device route");

        // 从 Online 服务查询设备信息
        let devices_resp = self
            .online_client
            .list_user_devices(user_id)
            .await
            .context("Failed to query user devices")?;

        // 查找指定设备
        let device = devices_resp
            .devices
            .into_iter()
            .find(|d| d.device_id == device_id);

        match device {
            Some(d) => {
                let route = DeviceRoute::new(
                    user_id.to_string(),
                    d.device_id,
                    d.gateway_id,
                    d.server_id,
                    d.priority,
                    calculate_quality_score(&d.connection_quality),
                );
                Ok(Some(route))
            }
            None => {
                info!(user_id = %user_id, device_id = %device_id, "Device not found");
                Ok(None)
            }
        }
    }

    /// 批量获取设备路由
    ///
    /// # 参数
    /// * `devices` - 设备列表（user_id, device_id）
    ///
    /// # 返回
    /// 设备路由映射（key: "user_id:device_id"）
    pub async fn batch_get_device_routes(
        &self,
        devices: Vec<(String, String)>,
    ) -> Result<std::collections::HashMap<String, DeviceRoute>> {
        info!(device_count = devices.len(), "Batch getting device routes");

        let mut routes = std::collections::HashMap::new();

        // 按用户分组查询（减少 RPC 调用次数）
        let mut user_devices: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for (user_id, device_id) in devices {
            user_devices
                .entry(user_id)
                .or_insert_with(Vec::new)
                .push(device_id);
        }

        // 为每个用户查询设备
        for (user_id, device_ids) in user_devices {
            match self.online_client.list_user_devices(&user_id).await {
                Ok(devices_resp) => {
                    for device in devices_resp.devices {
                        if device_ids.contains(&device.device_id) {
                            let key = format!("{}:{}", user_id, device.device_id);
                            let route = DeviceRoute::new(
                                user_id.clone(),
                                device.device_id,
                                device.gateway_id,
                                device.server_id,
                                device.priority,
                                calculate_quality_score(&device.connection_quality),
                            );
                            routes.insert(key, route);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, user_id = %user_id, "Failed to query devices for user");
                    continue;
                }
            }
        }

        info!(found_routes = routes.len(), "Batch device routes retrieved");
        Ok(routes)
    }
}

/// 计算链接质量评分 (0-100)
fn calculate_quality_score(quality: &Option<flare_proto::ConnectionQuality>) -> f64 {
    match quality {
        Some(q) => {
            // 综合考虑 RTT 和丢包率
            let rtt_score = if q.rtt_ms > 0 {
                (1000.0_f64 / q.rtt_ms as f64).min(100.0_f64)
            } else {
                100.0
            };

            let loss_score = (1.0 - q.packet_loss_rate) * 100.0;

            // RTT 权重 60%，丢包率权重 40%
            rtt_score * 0.6 + loss_score * 0.4
        }
        None => 50.0, // 默认中等质量
    }
}

