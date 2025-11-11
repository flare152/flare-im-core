use std::env;

#[derive(Clone, Debug)]
pub struct StorageWriterConfig {
    pub kafka_bootstrap: String,
    pub kafka_topic: String,
    pub kafka_group: String,
    pub kafka_ack_topic: Option<String>,
    pub kafka_timeout_ms: u64,
    pub redis_url: Option<String>,
    pub redis_hot_ttl_seconds: u64,
    pub redis_idempotency_ttl_seconds: u64,
    pub wal_hash_key: Option<String>,
    pub postgres_url: Option<String>,
    pub mongo_url: Option<String>,
    pub mongo_database: String,
    pub mongo_collection: String,
    pub media_service_endpoint: Option<String>,
}

impl StorageWriterConfig {
    pub fn from_env() -> Self {
        let kafka_bootstrap = env::var("STORAGE_KAFKA_BOOTSTRAP_SERVERS")
            .unwrap_or_else(|_| "localhost:9092".to_string());
        let kafka_topic = env::var("STORAGE_KAFKA_STORAGE_TOPIC")
            .unwrap_or_else(|_| "storage-messages".to_string());
        let kafka_group = env::var("STORAGE_KAFKA_STORAGE_GROUP")
            .unwrap_or_else(|_| "storage-writer".to_string());
        let kafka_ack_topic = env::var("STORAGE_KAFKA_ACK_TOPIC").ok();
        let kafka_timeout_ms = env::var("STORAGE_KAFKA_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(5000);

        let redis_url = env::var("STORAGE_REDIS_URL").ok();
        let redis_hot_ttl_seconds = env::var("STORAGE_REDIS_HOT_TTL_SECONDS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(7 * 24 * 3600);
        let redis_idempotency_ttl_seconds = env::var("STORAGE_REDIS_IDEMPOTENCY_TTL_SECONDS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(24 * 3600);
        let wal_hash_key = env::var("STORAGE_WAL_HASH_KEY")
            .ok()
            .or_else(|| redis_url.as_ref().map(|_| "storage:wal:buffer".to_string()));

        let postgres_url = env::var("STORAGE_POSTGRES_URL").ok();
        let mongo_url = env::var("STORAGE_MONGO_URL").ok();
        let mongo_database =
            env::var("STORAGE_MONGO_DATABASE").unwrap_or_else(|_| "flare_im".to_string());
        let mongo_collection =
            env::var("STORAGE_MONGO_MESSAGE_COLLECTION").unwrap_or_else(|_| "messages".to_string());
        let media_service_endpoint = env::var("MEDIA_SERVICE_ENDPOINT").ok();

        Self {
            kafka_bootstrap,
            kafka_topic,
            kafka_group,
            kafka_ack_topic,
            kafka_timeout_ms,
            redis_url,
            redis_hot_ttl_seconds,
            redis_idempotency_ttl_seconds,
            wal_hash_key,
            postgres_url,
            mongo_url,
            mongo_database,
            mongo_collection,
            media_service_endpoint,
        }
    }
}
