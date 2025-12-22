use std::collections::HashMap;
use std::sync::Arc;

use flare_proto::signaling::router::router_service_server::RouterService;
use flare_proto::signaling::router::*;
use flare_server_core::error::ErrorCode;
use tonic::{Request, Response, Status};
use tracing::{error, info, warn};

use crate::infrastructure::{OnlineServiceClient, forwarder::MessageForwarder};
use crate::util;

/// Router 服务 gRPC 处理器
///
/// 职责：
/// - 根据推送策略选择最优设备
/// - 提供设备路由查询能力
/// - 路由消息到业务系统（根据 SVID）
/// - 无状态服务，所有数据实时从 Online 服务查询
#[derive(Clone)]
pub struct RouteHandler {
    online_client: Arc<OnlineServiceClient>,
    message_forwarder: Arc<MessageForwarder>,
}

impl RouteHandler {
    pub fn new(online_client: Arc<OnlineServiceClient>, message_forwarder: Arc<MessageForwarder>) -> Self {
        Self { 
            online_client,
            message_forwarder,
        }
    }

    /// 根据策略选择推送目标设备
    fn select_targets_by_strategy(
        &self,
        devices: Vec<flare_proto::signaling::online::DeviceInfo>,
        strategy: PushStrategy,
        user_id: &str,
    ) -> Vec<RouteTarget> {
        if devices.is_empty() {
            return vec![];
        }

        let mut targets: Vec<RouteTarget> = devices
            .into_iter()
            .map(|d| RouteTarget {
                user_id: user_id.to_string(),
                device_id: d.device_id,
                device_platform: d.platform,
                gateway_id: d.gateway_id,
                server_id: d.server_id,
                priority: d.priority,
                quality_score: calculate_quality_score(&d.connection_quality),
            })
            .collect();

        match strategy {
            PushStrategy::AllDevices => targets,
            PushStrategy::BestDevice => {
                // 选择单个最优设备（优先级最高 + 质量最好）
                targets.sort_by(|a, b| {
                    b.priority.cmp(&a.priority).then_with(|| {
                        b.quality_score
                            .partial_cmp(&a.quality_score)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                });
                targets.into_iter().take(1).collect()
            }
            PushStrategy::ActiveDevices => {
                // 排除 Low 优先级设备 (priority = 0)
                targets.retain(|t| t.priority > 0);
                targets
            }
            PushStrategy::PrimaryDevice => {
                // 只选择优先级最高的设备
                if let Some(max_priority) = targets.iter().map(|t| t.priority).max() {
                    targets.retain(|t| t.priority == max_priority);
                }
                targets.into_iter().take(1).collect()
            }
            _ => targets, // 默认返回所有设备
        }
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

#[tonic::async_trait]
impl RouterService for RouteHandler {
    async fn select_push_targets(
        &self,
        request: Request<SelectPushTargetsRequest>,
    ) -> std::result::Result<Response<SelectPushTargetsResponse>, Status> {
        let req = request.into_inner();
        let user_id = &req.user_id;
        let strategy = PushStrategy::try_from(req.strategy).unwrap_or(PushStrategy::AllDevices);

        info!(user_id = %user_id, strategy = ?strategy, "Selecting push targets");

        // 从 Online 服务查询用户的所有在线设备
        let devices_resp = match self.online_client.list_user_devices(user_id).await {
            Ok(resp) => resp,
            Err(e) => {
                error!(error = %e, user_id = %user_id, "Failed to query user devices from Online service");
                return Ok(Response::new(SelectPushTargetsResponse {
                    targets: vec![],
                    status: util::rpc_status_error(
                        ErrorCode::InternalError,
                        &format!("Failed to query devices: {}", e),
                    ),
                }));
            }
        };

        // 根据策略选择目标设备
        let targets = self.select_targets_by_strategy(devices_resp.devices, strategy, user_id);

        info!(
            user_id = %user_id,
            strategy = ?strategy,
            selected_count = targets.len(),
            "Push targets selected"
        );

        Ok(Response::new(SelectPushTargetsResponse {
            targets,
            status: util::rpc_status_ok(),
        }))
    }

    async fn get_device_route(
        &self,
        request: Request<GetDeviceRouteRequest>,
    ) -> std::result::Result<Response<GetDeviceRouteResponse>, Status> {
        let req = request.into_inner();
        let user_id = &req.user_id;
        let device_id = &req.device_id;

        info!(user_id = %user_id, device_id = %device_id, "Getting device route");

        // 从 Online 服务查询设备信息
        let devices_resp = match self.online_client.list_user_devices(user_id).await {
            Ok(resp) => resp,
            Err(e) => {
                error!(error = %e, user_id = %user_id, "Failed to query user devices");
                return Ok(Response::new(GetDeviceRouteResponse {
                    route: None,
                    status: util::rpc_status_error(
                        ErrorCode::InternalError,
                        &format!("Failed to query devices: {}", e),
                    ),
                }));
            }
        };

        // 查找指定设备
        let device = devices_resp
            .devices
            .into_iter()
            .find(|d| d.device_id == *device_id);

        match device {
            Some(d) => {
                let route = RouteTarget {
                    user_id: user_id.to_string(),
                    device_id: d.device_id,
                    device_platform: d.platform,
                    gateway_id: d.gateway_id,
                    server_id: d.server_id,
                    priority: d.priority,
                    quality_score: calculate_quality_score(&d.connection_quality),
                };

                Ok(Response::new(GetDeviceRouteResponse {
                    route: Some(route),
                    status: util::rpc_status_ok(),
                }))
            }
            None => {
                warn!(user_id = %user_id, device_id = %device_id, "Device not found");
                Ok(Response::new(GetDeviceRouteResponse {
                    route: None,
                    status: util::rpc_status_error(
                        ErrorCode::UserNotFound,
                        "Device not found or offline",
                    ),
                }))
            }
        }
    }

    async fn batch_get_device_routes(
        &self,
        request: Request<BatchGetDeviceRoutesRequest>,
    ) -> std::result::Result<Response<BatchGetDeviceRoutesResponse>, Status> {
        let req = request.into_inner();
        let device_count = req.devices.len();

        info!(device_count = device_count, "Batch getting device routes");

        let mut routes = HashMap::new();

        // 按用户分组查询（减少 RPC 调用次数）
        let mut user_devices: HashMap<String, Vec<String>> = HashMap::new();
        for dev in req.devices {
            user_devices
                .entry(dev.user_id)
                .or_insert_with(Vec::new)
                .push(dev.device_id);
        }

        // 为每个用户查询设备
        for (user_id, device_ids) in user_devices {
            match self.online_client.list_user_devices(&user_id).await {
                Ok(devices_resp) => {
                    for device in devices_resp.devices {
                        if device_ids.contains(&device.device_id) {
                            let key = format!("{}:{}", user_id, device.device_id);
                            let route = RouteTarget {
                                user_id: user_id.clone(),
                                device_id: device.device_id,
                                device_platform: device.platform,
                                gateway_id: device.gateway_id,
                                server_id: device.server_id,
                                priority: device.priority,
                                quality_score: calculate_quality_score(&device.connection_quality),
                            };
                            routes.insert(key, route);
                        }
                    }
                }
                Err(e) => {
                    warn!(error = %e, user_id = %user_id, "Failed to query devices for user");
                    continue;
                }
            }
        }

        info!(found_routes = routes.len(), "Batch device routes retrieved");

        Ok(Response::new(BatchGetDeviceRoutesResponse {
            routes,
            status: util::rpc_status_ok(),
        }))
    }

    async fn route_message(
        &self,
        request: Request<RouteMessageRequest>,
    ) -> std::result::Result<Response<RouteMessageResponse>, Status> {
        use std::time::Instant;
        use flare_proto::signaling::router::{RouteMetadata, RouteOptions};

        let req = request.into_inner();
        let svid = &req.svid;
        let start_time = Instant::now();
        let decision_start = Instant::now();

        // 提取路由选项
        let route_options = req.options.unwrap_or_else(|| RouteOptions {
            timeout_seconds: 5,
            enable_tracing: true,
            retry_strategy: 0, // RETRY_STRATEGY_NONE
            load_balance_strategy: 1, // LOAD_BALANCE_STRATEGY_ROUND_ROBIN
            priority: 0,
        });

        info!(
            svid = %svid,
            payload_len = req.payload.len(),
            timeout_seconds = route_options.timeout_seconds,
            enable_tracing = route_options.enable_tracing,
            "Routing message to business system"
        );

        // 传播追踪信息（如果启用）
        let trace_context = if route_options.enable_tracing {
            req.context
                .as_ref()
                .and_then(|ctx| ctx.trace.clone())
        } else {
            None
        };

        // 记录路由决策耗时
        let decision_duration = decision_start.elapsed();

        // 使用 MessageForwarder 转发消息
        let business_start = Instant::now();
        let forward_result: Result<(String, Vec<u8>), anyhow::Error> = self
            .message_forwarder
            .forward_message(
                svid,
                req.payload,
                req.context,
                req.tenant,
                None, // route_repository 暂时为 None，后续可以注入
            )
            .await;

        let business_duration = business_start.elapsed();
        let total_duration = start_time.elapsed();

        let (response_data, routed_endpoint) = match forward_result {
            Ok((endpoint, data)) => {
                info!(
                    svid = %svid,
                    routed_endpoint = %endpoint,
                    response_len = %data.len(),
                    decision_duration_ms = decision_duration.as_millis(),
                    business_duration_ms = business_duration.as_millis(),
                    total_duration_ms = total_duration.as_millis(),
                    "Message routed successfully"
                );
                (data, endpoint)
            }
            Err(e) => {
                error!(
                    error = %e,
                    svid = %svid,
                    decision_duration_ms = decision_duration.as_millis(),
                    total_duration_ms = total_duration.as_millis(),
                    "Failed to forward message to business system"
                );
                return Ok(Response::new(RouteMessageResponse {
                    response_data: vec![],
                    routed_endpoint: String::new(),
                    metadata: Some(RouteMetadata {
                        route_duration_ms: total_duration.as_millis() as i64,
                        business_duration_ms: 0,
                        decision_duration_ms: decision_duration.as_millis() as i64,
                        from_cache: false,
                        decision_details: std::collections::HashMap::new(),
                        trace: trace_context.clone(),
                    }),
                    status: util::rpc_status_error(
                        ErrorCode::InternalError,
                        &format!("Failed to forward message: {}", e),
                    ),
                }));
            }
        };

        // 构建路由元数据
        let metadata = RouteMetadata {
            route_duration_ms: total_duration.as_millis() as i64,
            business_duration_ms: business_duration.as_millis() as i64,
            decision_duration_ms: decision_duration.as_millis() as i64,
            from_cache: false, // 后续可以从缓存状态中获取
            decision_details: {
                let mut details = std::collections::HashMap::new();
                details.insert("svid".to_string(), svid.clone());
                details.insert("load_balance_strategy".to_string(), format!("{}", route_options.load_balance_strategy));
                details
            },
            trace: trace_context,
        };

        Ok(Response::new(RouteMessageResponse {
            response_data,
            routed_endpoint,
            metadata: Some(metadata),
            status: util::rpc_status_ok(),
        }))
    }
}
