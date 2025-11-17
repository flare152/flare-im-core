use anyhow::Result;
use flare_im_core::config::FlareAppConfig;
use std::env;

#[derive(Debug, Clone)]
pub struct GatewayConfig {
    pub signaling_endpoint: String,
    pub push_endpoint: String,
    pub message_endpoint: String,
    pub media_endpoint: String,
    pub hook_engine_endpoint: String,
}

impl GatewayConfig {
    /// 从应用配置加载（新方式，推荐）
    pub fn from_app_config(_app: &FlareAppConfig) -> Result<Self> {
        // 网关服务端点通过环境变量配置（支持服务发现）
        Ok(Self {
            signaling_endpoint: env::var("SIGNALING_SERVICE_ENDPOINT")
                .unwrap_or_else(|_| "http://localhost:50061".to_string()),
            push_endpoint: env::var("PUSH_SERVICE_ENDPOINT")
                .unwrap_or_else(|_| "http://localhost:50071".to_string()),
            message_endpoint: env::var("MESSAGE_ORCHESTRATOR_ENDPOINT")
                .or_else(|_| env::var("STORAGE_SERVICE_ENDPOINT"))
                .unwrap_or_else(|_| "http://localhost:50081".to_string()),
            media_endpoint: env::var("MEDIA_SERVICE_ENDPOINT")
                .unwrap_or_else(|_| "http://localhost:50091".to_string()),
            hook_engine_endpoint: env::var("HOOK_ENGINE_ENDPOINT")
                .unwrap_or_else(|_| "http://localhost:50110".to_string()),
        })
    }

    /// 从环境变量加载（保留用于向后兼容，但不推荐使用）
    #[deprecated(note = "Use from_app_config instead")]
    pub fn from_env() -> Self {
        Self {
            signaling_endpoint: env::var("SIGNALING_SERVICE_ENDPOINT")
                .unwrap_or_else(|_| "http://localhost:50061".to_string()),
            push_endpoint: env::var("PUSH_SERVICE_ENDPOINT")
                .unwrap_or_else(|_| "http://localhost:50071".to_string()),
            message_endpoint: env::var("MESSAGE_ORCHESTRATOR_ENDPOINT")
                .or_else(|_| env::var("STORAGE_SERVICE_ENDPOINT"))
                .unwrap_or_else(|_| "http://localhost:50081".to_string()),
            media_endpoint: env::var("MEDIA_SERVICE_ENDPOINT")
                .unwrap_or_else(|_| "http://localhost:50091".to_string()),
            hook_engine_endpoint: env::var("HOOK_ENGINE_ENDPOINT")
                .unwrap_or_else(|_| "http://localhost:50110".to_string()),
        }
    }
}
