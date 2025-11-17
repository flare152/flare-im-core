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
    pub online_cache_ttl_seconds: u64,
    pub online_cache_max_size: usize,
    // ACK上报配置
    pub kafka_bootstrap: Option<String>,
    pub ack_topic: Option<String>,
    // 跨地区网关路由配置
    pub gateway_id: Option<String>,
    pub region: Option<String>,
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

        // 在线状态缓存配置（默认30秒TTL，最大10万条目）
        let online_cache_ttl_seconds = std::env::var("ACCESS_GATEWAY_ONLINE_CACHE_TTL_SECONDS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(30);

        let online_cache_max_size = std::env::var("ACCESS_GATEWAY_ONLINE_CACHE_MAX_SIZE")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(100_000);

        // ACK上报配置
        let kafka_bootstrap = std::env::var("ACCESS_GATEWAY_KAFKA_BOOTSTRAP_SERVERS")
            .ok()
            .or_else(|| {
                app.kafka_profile("default")
                    .map(|profile| profile.bootstrap_servers.clone())
            });
        
        let ack_topic = std::env::var("ACCESS_GATEWAY_ACK_TOPIC")
            .ok()
            .or_else(|| Some("flare.im.push.ack".to_string()));

        // 跨地区网关路由配置
        let gateway_id = std::env::var("GATEWAY_ID")
            .ok()
            .or_else(|| service.gateway_id.clone());
        
        let region = std::env::var("GATEWAY_REGION")
            .ok()
            .or_else(|| service.region.clone());

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
            online_cache_ttl_seconds,
            online_cache_max_size,
            kafka_bootstrap,
            ack_topic,
            gateway_id,
            region,
        }
    }
}
