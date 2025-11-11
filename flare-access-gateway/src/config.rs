use flare_im_core::config::{FlareAppConfig, RedisPoolConfig};

#[derive(Debug, Clone)]
pub struct AccessGatewayConfig {
    pub signaling_endpoint: String,
    pub message_endpoint: String,
    pub push_endpoint: String,
    pub token_secret: String,
    pub token_issuer: String,
    pub token_ttl_seconds: u64,
    pub token_store_redis_url: Option<String>,
    pub session_store_redis_url: Option<String>,
    pub session_store_ttl_seconds: u64,
}

impl AccessGatewayConfig {
    pub fn from_app_config(app: &FlareAppConfig) -> Self {
        let service = app.access_gateway_service();

        let session_profile: Option<RedisPoolConfig> = service
            .session_store
            .as_deref()
            .and_then(|name| app.redis_profile(name))
            .cloned();

        let token_profile: Option<RedisPoolConfig> = service
            .token_store
            .as_deref()
            .and_then(|name| app.redis_profile(name))
            .cloned();

        let signaling_endpoint = service
            .signaling_endpoint
            .unwrap_or_else(|| "http://localhost:50061".to_string());

        let message_endpoint = service
            .message_endpoint
            .unwrap_or_else(|| "http://localhost:50081".to_string());

        let push_endpoint = service
            .push_endpoint
            .unwrap_or_else(|| "http://localhost:50091".to_string());

        let token_secret = service
            .token_secret
            .unwrap_or_else(|| "insecure-secret".to_string());

        let token_issuer = service
            .token_issuer
            .unwrap_or_else(|| "flare-im-core".to_string());

        let token_ttl_seconds = service.token_ttl_seconds.unwrap_or(3600);

        let session_store_ttl_seconds = service
            .session_store_ttl_seconds
            .or_else(|| session_profile.as_ref().and_then(|p| p.ttl_seconds))
            .unwrap_or(3600);

        Self {
            signaling_endpoint,
            message_endpoint,
            push_endpoint,
            token_secret,
            token_issuer,
            token_ttl_seconds,
            token_store_redis_url: token_profile.as_ref().map(|p| p.url.clone()),
            session_store_redis_url: session_profile.as_ref().map(|p| p.url.clone()),
            session_store_ttl_seconds,
        }
    }
}
