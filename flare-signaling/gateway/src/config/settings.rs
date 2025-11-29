use flare_im_core::config::{FlareAppConfig, RedisPoolConfig};
use flare_server_core::kafka::KafkaProducerConfig;

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
    // 消息大小限制配置
    pub max_message_size_bytes: usize,  // 最大消息大小（字节），默认 10MB
    pub max_text_content_length: usize, // 最大文本内容长度（字符），默认 100KB
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

        // 消息大小限制配置（从环境变量或使用默认值）
        let max_message_size_bytes = std::env::var("GATEWAY_MAX_MESSAGE_SIZE_BYTES")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(10 * 1024 * 1024); // 默认 10MB

        let max_text_content_length = std::env::var("GATEWAY_MAX_TEXT_CONTENT_LENGTH")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(100 * 1024); // 默认 100KB 字符

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
            session_store_redis_url: session_profile.as_ref().map(|p| p.url.clone()),
            session_store_ttl_seconds,
            online_cache_ttl_seconds,
            online_cache_max_size,
            kafka_bootstrap,
            ack_topic,
            gateway_id,
            region,
            max_message_size_bytes,
            max_text_content_length,
        }
    }
}

// 实现 KafkaProducerConfig trait，使 AccessGatewayConfig 可以使用通用的 Kafka 生产者构建器
// 注意：kafka_bootstrap 是 Option，需要在使用时检查
impl KafkaProducerConfig for AccessGatewayConfig {
    fn kafka_bootstrap(&self) -> &str {
        // 如果 kafka_bootstrap 为 None，返回默认值（但实际使用时应该先检查）
        self.kafka_bootstrap.as_deref().unwrap_or("localhost:9092")
    }
    
    fn message_timeout_ms(&self) -> u64 {
        5000 // 默认 5 秒
    }
    
    // 使用默认值，或根据需要覆盖
    fn enable_idempotence(&self) -> bool {
        true // ACK 消息需要保证不丢失
    }
    
    fn compression_type(&self) -> &str {
        "snappy" // 使用 snappy 压缩
    }
}
