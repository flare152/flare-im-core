pub mod route_domain_service;
pub mod message_routing_service;

pub use route_domain_service::RouteDomainService;
pub use message_routing_service::MessageRoutingDomainService;

/// 路由上下文值对象
#[derive(Debug, Clone, Default)]
pub struct RouteContext {
    pub svid: String,
    pub conversation_id: Option<String>,
    pub user_id: Option<String>,
    pub tenant_id: Option<String>,
    pub client_geo: Option<String>,
    pub login_gateway: Option<String>,
}

// 重新导出旧的领域服务（用于向后兼容）
#[deprecated(note = "Use RouteDomainService instead")]
pub use route_domain_service::RouteDomainService as RouteDomainServiceLegacy;

