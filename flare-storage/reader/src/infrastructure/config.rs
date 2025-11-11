use std::env;

#[derive(Clone, Debug)]
pub struct StorageReaderConfig {
    pub redis_url: Option<String>,
    pub mongo_url: Option<String>,
    pub mongo_database: String,
    pub mongo_collection: String,
    pub postgres_url: Option<String>,
    pub default_range_seconds: i64,
    pub max_page_size: i32,
}

impl StorageReaderConfig {
    pub fn from_env() -> Self {
        let redis_url = env::var("STORAGE_REDIS_URL").ok();
        let mongo_url = env::var("STORAGE_MONGO_URL").ok();
        let mongo_database =
            env::var("STORAGE_MONGO_DATABASE").unwrap_or_else(|_| "flare_im".to_string());
        let mongo_collection =
            env::var("STORAGE_MONGO_MESSAGE_COLLECTION").unwrap_or_else(|_| "messages".to_string());
        let postgres_url = env::var("STORAGE_POSTGRES_URL").ok();
        let default_range_seconds = env::var("STORAGE_READER_DEFAULT_RANGE_SECONDS")
            .ok()
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(7 * 24 * 3600);
        let max_page_size = env::var("STORAGE_READER_MAX_PAGE_SIZE")
            .ok()
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(200);

        Self {
            redis_url,
            mongo_url,
            mongo_database,
            mongo_collection,
            postgres_url,
            default_range_seconds,
            max_page_size,
        }
    }
}
