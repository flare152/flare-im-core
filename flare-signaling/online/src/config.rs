use std::env;

#[derive(Debug, Clone)]
pub struct OnlineConfig {
    pub redis_url: String,
    pub redis_ttl_seconds: u64,
}

impl OnlineConfig {
    pub fn from_env() -> Self {
        Self {
            redis_url: env::var("SIGNALING_ONLINE_REDIS_URL")
                .unwrap_or_else(|_| "redis://127.0.0.1/".to_string()),
            redis_ttl_seconds: env::var("SIGNALING_ONLINE_REDIS_TTL")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(3600),
        }
    }
}
