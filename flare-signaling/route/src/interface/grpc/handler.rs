use std::sync::Arc;

use flare_proto::signaling::router::router_service_server::RouterService;
use flare_proto::signaling::router::*;
use flare_server_core::error::ErrorCode;
use flare_server_core::middleware::context::extract_context;
use tonic::{Request, Response, Status};
use tracing::debug;

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

}

#[tonic::async_trait]
impl RouterService for RouteHandler {
    async fn select_push_targets(
        &self,
        request: Request<SelectPushTargetsRequest>,
    ) -> std::result::Result<Response<SelectPushTargetsResponse>, Status> {
        // 从请求扩展中提取 Context
        let ctx = extract_context(&request)
            .map_err(|e| Status::invalid_argument(format!("Context is required: {}", e)))?;

        let req = request.into_inner();
        let user_id = &req.user_id;
        let strategy = PushStrategy::try_from(req.strategy).unwrap_or(PushStrategy::AllDevices);

        // 通过 Application 层调用
        let result = self
            .device_route_handler
            .select_push_targets(&ctx, user_id, strategy)
            .await;

        Ok(Response::new(SelectPushTargetsResponse {
            targets: result.targets,
            status: if result.error_code.is_some() {
                let code = result.error_code.unwrap_or(ErrorCode::InternalError as u32);
                util::rpc_status_error(
                    ErrorCode::from_u32(code).unwrap_or(ErrorCode::InternalError),
                    &result.error_message.unwrap_or_default(),
                )
            } else {
                util::rpc_status_ok()
            },
        }))
    }

    async fn get_device_route(
        &self,
        request: Request<GetDeviceRouteRequest>,
    ) -> std::result::Result<Response<GetDeviceRouteResponse>, Status> {
        // 从请求扩展中提取 Context
        let ctx = extract_context(&request)
            .map_err(|e| Status::invalid_argument(format!("Context is required: {}", e)))?;

        let req = request.into_inner();
        let user_id = &req.user_id;
        let device_id = &req.device_id;

        // 通过 Application 层调用
        let result = self
            .device_route_handler
            .get_device_route(&ctx, user_id, device_id)
            .await;

        Ok(Response::new(GetDeviceRouteResponse {
            route: result.target,
            status: if result.error_code.is_some() {
                let code = result.error_code.unwrap_or(ErrorCode::InternalError as u32);
                util::rpc_status_error(
                    ErrorCode::from_u32(code).unwrap_or(ErrorCode::InternalError),
                    &result.error_message.unwrap_or_default(),
                )
            } else {
                util::rpc_status_ok()
            },
        }))
    }

    async fn batch_get_device_routes(
        &self,
        request: Request<BatchGetDeviceRoutesRequest>,
    ) -> std::result::Result<Response<BatchGetDeviceRoutesResponse>, Status> {
        // 从请求扩展中提取 Context
        let ctx = extract_context(&request)
            .map_err(|e| Status::invalid_argument(format!("Context is required: {}", e)))?;

        let req = request.into_inner();

        // 通过 Application 层调用
        let devices: Vec<(String, String)> = req
            .devices
            .into_iter()
            .map(|d| (d.user_id, d.device_id))
            .collect();

        let result = self
            .device_route_handler
            .batch_get_device_routes(&ctx, devices)
            .await;

        Ok(Response::new(BatchGetDeviceRoutesResponse {
            routes: result.routes,
            status: if result.error_code.is_some() {
                let code = result.error_code.unwrap_or(ErrorCode::InternalError as u32);
                util::rpc_status_error(
                    ErrorCode::from_u32(code).unwrap_or(ErrorCode::InternalError),
                    &result.error_message.unwrap_or_default(),
                )
            } else {
                util::rpc_status_ok()
            },
        }))
    }

    async fn route_message(
        &self,
        request: Request<RouteMessageRequest>,
    ) -> std::result::Result<Response<RouteMessageResponse>, Status> {
        use flare_proto::signaling::router::RouteOptions;

        // 从请求扩展中提取 Context（在移动 request 之前）
        let ctx = extract_context(&request)
            .map_err(|e| Status::invalid_argument(format!("Context is required: {}", e)))?;

        let req = request.into_inner();
        let svid = &req.svid;

        // 提取路由选项（使用默认值如果未提供）
        let route_options = req.options.unwrap_or_else(|| RouteOptions {
            timeout_seconds: 5,
            enable_tracing: true,
            retry_strategy: 0, // RETRY_STRATEGY_NONE
            load_balance_strategy: 1, // LOAD_BALANCE_STRATEGY_ROUND_ROBIN
            priority: 0,
        });

        debug!(
            svid = %svid,
            payload_len = req.payload.len(),
            timeout_seconds = route_options.timeout_seconds,
            enable_tracing = route_options.enable_tracing,
            "Routing message to business system"
        );

        // 通过 Application 层调用（业务逻辑已在应用层处理，包括元数据构建）
        let result = self
            .message_routing_handler
            .route_message(&ctx, svid, req.payload, route_options)
            .await;

        Ok(Response::new(RouteMessageResponse {
            response_data: result.response_data,
            routed_endpoint: result.routed_endpoint,
            metadata: Some(result.metadata),
            status: if result.error_code.is_some() {
                let code = result.error_code.unwrap_or(ErrorCode::InternalError as u32);
                util::rpc_status_error(
                    ErrorCode::from_u32(code).unwrap_or(ErrorCode::InternalError),
                    &result.error_message.unwrap_or_default(),
                )
            } else {
                util::rpc_status_ok()
            },
        }))
    }
}
