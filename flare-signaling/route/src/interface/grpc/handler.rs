use std::collections::HashMap;
use std::sync::Arc;

use flare_proto::signaling::router::router_service_server::RouterService;
use flare_proto::signaling::router::*;
use flare_server_core::error::ErrorCode;
use tonic::{Request, Response, Status};
use tracing::{error, info, warn};

use crate::application::handlers::{
    DeviceRouteHandler, MessageRoutingHandler,
};
use crate::util;

/// Router 服务 gRPC 处理器
///
/// 职责：
/// - 根据推送策略选择最优设备
/// - 提供设备路由查询能力
/// - 路由消息到业务系统（根据 SVID）
/// - 无状态服务，所有数据实时从 Online 服务查询
///
/// # DDD + CQRS 架构
/// Interface 层通过 Application 层调用，不直接访问 Infrastructure 层
#[derive(Clone)]
pub struct RouteHandler {
    device_route_handler: Arc<DeviceRouteHandler>,
    message_routing_handler: Arc<MessageRoutingHandler>,
}

impl RouteHandler {
    pub fn new(
        device_route_handler: Arc<DeviceRouteHandler>,
        message_routing_handler: Arc<MessageRoutingHandler>,
    ) -> Self {
        Self {
            device_route_handler,
            message_routing_handler,
        }
    }

    /// 将 DeviceRoute 转换为 RouteTarget（protobuf 类型）
    fn device_route_to_target(route: &crate::domain::entities::device_route::DeviceRoute) -> RouteTarget {
        // 注意：DeviceRoute 中没有 device_platform 字段，需要从 Online 服务获取
        // 这里简化处理，使用默认值
        RouteTarget {
            user_id: route.user_id.clone(),
            device_id: route.device_id.clone(),
            device_platform: String::new(), // TODO: 从 Online 服务获取 platform 信息
            gateway_id: route.gateway_id.clone(),
            server_id: route.server_id.clone(),
            priority: route.device_priority,
            quality_score: route.quality_score,
        }
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

        // 通过 Application 层调用
        let routes = match self
            .device_route_handler
            .select_push_targets(user_id, strategy)
            .await
        {
            Ok(routes) => routes,
            Err(e) => {
                error!(error = %e, user_id = %user_id, "Failed to select push targets");
                return Ok(Response::new(SelectPushTargetsResponse {
                    targets: vec![],
                    status: util::rpc_status_error(
                        ErrorCode::InternalError,
                        &format!("Failed to select push targets: {}", e),
                    ),
                }));
            }
        };

        // 转换为 protobuf 类型
        let targets: Vec<RouteTarget> = routes
            .iter()
            .map(|r| Self::device_route_to_target(r))
            .collect();

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

        // 通过 Application 层调用
        match self
            .device_route_handler
            .get_device_route(user_id, device_id)
            .await
        {
            Ok(Some(route)) => {
                Ok(Response::new(GetDeviceRouteResponse {
                    route: Some(Self::device_route_to_target(&route)),
                    status: util::rpc_status_ok(),
                }))
            }
            Ok(None) => {
                warn!(user_id = %user_id, device_id = %device_id, "Device not found");
                Ok(Response::new(GetDeviceRouteResponse {
                    route: None,
                    status: util::rpc_status_error(
                        ErrorCode::UserNotFound,
                        "Device not found or offline",
                    ),
                }))
            }
            Err(e) => {
                error!(error = %e, user_id = %user_id, "Failed to get device route");
                Ok(Response::new(GetDeviceRouteResponse {
                    route: None,
                    status: util::rpc_status_error(
                        ErrorCode::InternalError,
                        &format!("Failed to get device route: {}", e),
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

        // 通过 Application 层调用
        let devices: Vec<(String, String)> = req
            .devices
            .into_iter()
            .map(|d| (d.user_id, d.device_id))
            .collect();

        let device_routes = match self
            .device_route_handler
            .batch_get_device_routes(devices)
            .await
        {
            Ok(routes) => routes,
            Err(e) => {
                error!(error = %e, "Failed to batch get device routes");
                return Ok(Response::new(BatchGetDeviceRoutesResponse {
                    routes: HashMap::new(),
                    status: util::rpc_status_error(
                        ErrorCode::InternalError,
                        &format!("Failed to batch get device routes: {}", e),
                    ),
                }));
            }
        };

        // 转换为 protobuf 类型
        let routes: HashMap<String, RouteTarget> = device_routes
            .into_iter()
            .map(|(k, v)| (k, Self::device_route_to_target(&v)))
            .collect();

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

        // 通过 Application 层调用
        let business_start = Instant::now();
        let forward_result = self
            .message_routing_handler
            .route_message(svid, req.payload, req.context, req.tenant)
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
