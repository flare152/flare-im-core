use anyhow::Result;
use flare_im_core::config::FlareAppConfig;
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
    /// 从应用配置加载（新方式，推荐）
    pub fn from_app_config(app: &FlareAppConfig) -> Result<Self> {
        let service_config = app.storage_reader_service();
        
        // 解析 MongoDB 配置引用
        let mongo_url = env::var("STORAGE_MONGO_URL")
            .ok()
            .or_else(|| {
                if let Some(mongo_name) = &service_config.mongo {
                    app.mongodb_profile(mongo_name)
                        .map(|profile| profile.url.clone())
                } else {
                    None
                }
            });
        
        let mongo_database = mongo_url
            .as_ref()
            .and_then(|_| {
                if let Some(mongo_name) = &service_config.mongo {
                    app.mongodb_profile(mongo_name)
                        .and_then(|profile| profile.database.clone())
                } else {
                    None
                }
            })
            .or_else(|| env::var("STORAGE_MONGO_DATABASE").ok())
            .unwrap_or_else(|| "flare_im".to_string());

        let mongo_collection = env::var("STORAGE_MONGO_MESSAGE_COLLECTION")
            .unwrap_or_else(|_| "messages".to_string());

        // 解析 Redis 配置引用（可选）
        let redis_url = env::var("STORAGE_REDIS_URL")
            .ok()
            .or_else(|| {
                if let Some(redis_name) = &service_config.redis {
                    app.redis_profile(redis_name)
                        .map(|profile| profile.url.clone())
                } else {
                    None
                }
            });

        let postgres_url = env::var("STORAGE_POSTGRES_URL").ok();

        let default_range_seconds = env::var("STORAGE_READER_DEFAULT_RANGE_SECONDS")
            .ok()
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(7 * 24 * 3600);

        let max_page_size = env::var("STORAGE_READER_MAX_PAGE_SIZE")
            .ok()
            .and_then(|v| v.parse::<i32>().ok())
            .or_else(|| service_config.max_page_size.map(|v| v as i32))
            .unwrap_or(200);

        Ok(Self {
            redis_url,
            mongo_url,
            mongo_database,
            mongo_collection,
            postgres_url,
            default_range_seconds,
            max_page_size,
        })
    }

    /// 从环境变量加载（保留用于向后兼容，但不推荐使用）
    #[deprecated(note = "Use from_app_config instead")]
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
