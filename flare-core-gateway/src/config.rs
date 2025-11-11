use std::env;

#[derive(Debug, Clone)]
pub struct GatewayConfig {
    pub signaling_endpoint: String,
    pub push_endpoint: String,
    pub message_endpoint: String,
    pub media_endpoint: String,
}

impl GatewayConfig {
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
        }
    }
}
