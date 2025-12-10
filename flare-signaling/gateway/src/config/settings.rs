use flare_im_core::config::{FlareAppConfig, RedisPoolConfig};

#[derive(Debug, Clone)]
pub struct AccessGatewayConfig {
    pub signaling_service: String,
    pub route_service: Option<String>,  // Route 服务（新增）
    pub message_service: String,
    pub push_service: String,
    pub default_svid: String,  // 默认 SVID（新增，默认 "svid.im"）
    pub use_route_service: bool,  // 是否使用 Route 服务（新增，默认 true）
    pub token_secret: String,
    pub token_issuer: String,
    pub token_ttl_seconds: u64,
    pub token_store_redis_url: Option<String>,
    // ACK上报配置（使用 gRPC，无需 Kafka）
    pub use_ack_report: bool,
    // 跨地区网关路由配置
    pub gateway_id: Option<String>,
    pub region: Option<String>,
}

impl AccessGatewayConfig {
    pub fn from_app_config(app: &FlareAppConfig) -> Self {
        let service = app.access_gateway_service();

        let token_profile: Option<RedisPoolConfig> = service
            .token_store
            .as_deref()
            .and_then(|name| app.redis_profile(name))
            .cloned();

        // 使用服务注册发现获取服务名（必须配置）
        // 注意：服务名必须与注册中心中的服务类型一致（不带 flare- 前缀）
        let signaling_service = service
            .signaling_service
            .or_else(|| service._signaling_endpoint.as_ref().and_then(|_| {
                // 如果使用旧配置，尝试从 endpoint 提取服务名（向后兼容）
                Some("signaling-online".to_string())
            }))
            .unwrap_or_else(|| "signaling-online".to_string());

        let message_service = service
            .message_service
            .or_else(|| service._message_endpoint.as_ref().and_then(|_| {
                Some("message-orchestrator".to_string())
            }))
            .unwrap_or_else(|| "message-orchestrator".to_string());

        let push_service = service
            .push_service
            .or_else(|| service._push_endpoint.as_ref().and_then(|_| {
                Some("push-server".to_string())
            }))
            .unwrap_or_else(|| "push-server".to_string());

        // Route 服务配置（新增）
        // Route 服务配置（新增）
        let route_service = service.route_service.clone();
        
        // 默认 SVID（新增）
        let default_svid = service.default_svid.clone()
            .or_else(|| std::env::var("ACCESS_GATEWAY_DEFAULT_SVID").ok())
            .unwrap_or_else(|| "svid.im".to_string());
        
        // 是否使用 Route 服务（新增，默认 true）
        let use_route_service = if let Some(use_route) = std::env::var("ACCESS_GATEWAY_USE_ROUTE_SERVICE")
            .ok()
            .and_then(|v| v.parse::<bool>().ok()) {
            use_route
        } else {
            service.use_route_service
        };

        let token_secret = service
            .token_secret
            .unwrap_or_else(|| "insecure-secret".to_string());

        let token_issuer = service
            .token_issuer
            .unwrap_or_else(|| "flare-im-core".to_string());

        let token_ttl_seconds = service.token_ttl_seconds.unwrap_or(3600);

        // ACK上报配置（使用 gRPC，默认开启）
        let use_ack_report = std::env::var("ACCESS_GATEWAY_USE_ACK_REPORT")
            .ok()
            .and_then(|v| v.parse::<bool>().ok())
            .unwrap_or(true);  // 默认开启

        // 跨地区网关路由配置
        let gateway_id = std::env::var("GATEWAY_ID")
            .ok()
            .or_else(|| service.gateway_id.clone());
        
        let region = std::env::var("GATEWAY_REGION")
            .ok()
            .or_else(|| service.region.clone());

        Self {
            signaling_service,
            route_service,
            message_service,
            push_service,
            default_svid,
            use_route_service,
            token_secret,
            token_issuer,
            token_ttl_seconds,
            token_store_redis_url: token_profile.as_ref().map(|p| p.url.clone()),
            use_ack_report,
            gateway_id,
            region,
        }
    }
}
