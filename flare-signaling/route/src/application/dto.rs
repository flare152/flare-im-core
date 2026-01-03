//! 应用层数据传输对象（DTO）
//!
//! 用于封装应用层到接口层的数据传输

use std::collections::HashMap;
use flare_proto::signaling::router::{RouteTarget, RouteMetadata};
use flare_proto::common::TraceContext;

/// 设备路由查询结果
#[derive(Debug, Clone)]
pub struct DeviceRouteResult {
    pub target: Option<RouteTarget>,
    pub error_code: Option<u32>,
    pub error_message: Option<String>,
}

/// 批量设备路由查询结果
#[derive(Debug, Clone)]
pub struct BatchDeviceRouteResult {
    pub routes: HashMap<String, RouteTarget>,
    pub error_code: Option<u32>,
    pub error_message: Option<String>,
}

/// 推送目标选择结果
#[derive(Debug, Clone)]
pub struct PushTargetsResult {
    pub targets: Vec<RouteTarget>,
    pub error_code: Option<u32>,
    pub error_message: Option<String>,
}

/// 消息路由结果
#[derive(Debug, Clone)]
pub struct MessageRouteResult {
    pub response_data: Vec<u8>,
    pub routed_endpoint: String,
    pub metadata: RouteMetadata,
    pub error_code: Option<u32>,
    pub error_message: Option<String>,
}

/// 工具函数：将领域实体转换为 RouteTarget
pub fn device_route_to_target(route: &crate::domain::entities::device_route::DeviceRoute) -> RouteTarget {
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

/// 工具函数：构建路由元数据
pub fn build_route_metadata(
    route_duration_ms: i64,
    business_duration_ms: i64,
    decision_duration_ms: i64,
    svid: &str,
    load_balance_strategy: i32,
    trace: Option<TraceContext>,
) -> RouteMetadata {
    RouteMetadata {
        route_duration_ms,
        business_duration_ms,
        decision_duration_ms,
        from_cache: false, // 后续可以从缓存状态中获取
        decision_details: {
            let mut details = HashMap::new();
            details.insert("svid".to_string(), svid.to_string());
            details.insert("load_balance_strategy".to_string(), format!("{}", load_balance_strategy));
            details
        },
        trace,
    }
}

