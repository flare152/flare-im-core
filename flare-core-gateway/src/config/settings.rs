use anyhow::Result;
use flare_im_core::config::FlareAppConfig;
use std::env;

#[derive(Debug, Clone)]
pub struct GatewayConfig {
    pub signaling_service: String,
    pub push_service: String,
    pub message_service: String,
    #[allow(dead_code)] // 保留用于未来扩展
    pub media_service: String,
    #[allow(dead_code)] // 保留用于未来扩展
    pub hook_engine_service: String,
    pub storage_service: String,
    pub access_gateway_service: String,
    pub route_service: String,
    pub use_route_service: bool,
    pub default_svid: String,
}

impl GatewayConfig {
    /// 从应用配置加载（新方式，推荐）
    pub fn from_app_config(app: &FlareAppConfig) -> Result<Self> {
        // 优先从配置文件加载服务名，如果没有则使用默认服务名
        let cfg = app.core_gateway_service();
        
        Ok(Self {
            // 注意：服务名必须与注册中心中的服务类型一致（不带 flare- 前缀）
            signaling_service: cfg.signaling_service
                .or_else(|| cfg._signaling_endpoint.as_ref().and_then(|_| Some("signaling-online".to_string())))
                .unwrap_or_else(|| "signaling-online".to_string()),
            push_service: cfg.push_service
                .or_else(|| cfg._push_endpoint.as_ref().and_then(|_| Some("push-server".to_string())))
                .unwrap_or_else(|| "push-server".to_string()),
            message_service: cfg.message_service
                .or_else(|| cfg._message_endpoint.as_ref().and_then(|_| Some("message-orchestrator".to_string())))
                .unwrap_or_else(|| "message-orchestrator".to_string()),
            storage_service: cfg.storage_service
                .or_else(|| cfg._storage_endpoint.as_ref().and_then(|_| Some("storage-reader".to_string())))
                .unwrap_or_else(|| "storage-reader".to_string()),
            media_service: cfg.media_service
                .or_else(|| cfg._media_endpoint.as_ref().and_then(|_| Some("media".to_string())))
                .unwrap_or_else(|| "media".to_string()),
            hook_engine_service: cfg.hook_engine_service
                .or_else(|| cfg._hook_engine_endpoint.as_ref().and_then(|_| Some("hook-engine".to_string())))
                .unwrap_or_else(|| "hook-engine".to_string()),
            access_gateway_service: "access-gateway".to_string(),
            route_service: cfg.route_service
                .unwrap_or_else(|| "signaling-route".to_string()),
            use_route_service: cfg.use_route_service.unwrap_or(false),
            default_svid: cfg.default_svid
                .unwrap_or_else(|| "svid.im".to_string()),
        })
    }

    /// 从环境变量加载（保留用于向后兼容，但不推荐使用）
    #[deprecated(note = "Use from_app_config instead")]
    #[allow(dead_code)] // 保留用于向后兼容
    pub fn from_env() -> Self {
        Self {
            access_gateway_service: env::var("ACCESS_GATEWAY_SERVICE")
                .unwrap_or_else(|_| "flare-access-gateway".to_string()),
            signaling_service: env::var("SIGNALING_SERVICE")
                .unwrap_or_else(|_| "flare-signaling-online".to_string()),
            push_service: env::var("PUSH_SERVICE")
                .unwrap_or_else(|_| "flare-push-server".to_string()),
            message_service: env::var("MESSAGE_SERVICE")
                .unwrap_or_else(|_| "flare-message-orchestrator".to_string()),
            storage_service: env::var("STORAGE_SERVICE")
                .unwrap_or_else(|_| "flare-storage-reader".to_string()),
            media_service: env::var("MEDIA_SERVICE")
                .unwrap_or_else(|_| "flare-media".to_string()),
            hook_engine_service: env::var("HOOK_ENGINE_SERVICE")
                .unwrap_or_else(|_| "flare-hook-engine".to_string()),
            route_service: env::var("ROUTE_SERVICE")
                .unwrap_or_else(|_| "signaling-route".to_string()),
            use_route_service: env::var("USE_ROUTE_SERVICE")
                .unwrap_or_else(|_| "false".to_string())
                .parse()
                .unwrap_or(false),
            default_svid: env::var("DEFAULT_SVID")
                .unwrap_or_else(|_| "svid.im".to_string()),
        }
    }
}
