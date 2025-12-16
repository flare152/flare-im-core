use anyhow::Result;
use flare_im_core::config::FlareAppConfig;
use std::collections::HashMap;
use std::env;

use crate::domain::model::{ConflictResolutionPolicy, SessionPolicy};

#[derive(Clone, Debug)]
pub struct SessionConfig {
    pub redis_url: String,
    pub postgres_url: Option<String>,
    pub session_state_prefix: String,
    pub session_unread_prefix: String,
    pub user_cursor_prefix: String,
    pub presence_prefix: String,
    pub storage_reader_service: Option<String>,
    pub recent_message_limit: i32,
    pub default_policy: SessionPolicy,
}

impl SessionConfig {
    /// 从应用配置加载（新方式，推荐）
    pub fn from_app_config(app: &FlareAppConfig) -> Result<Self> {
        let service_config = app.session_service();

        // 解析 Redis 配置引用
        let redis_url = env::var("SESSION_REDIS_URL")
            .or_else(|_| env::var("STORAGE_REDIS_URL"))
            .ok()
            .or_else(|| {
                if let Some(redis_name) = &service_config.redis {
                    app.redis_profile(redis_name)
                        .map(|profile| profile.url.clone())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "redis://127.0.0.1:6379/0".to_string());

        let postgres_url = env::var("SESSION_POSTGRES_URL").ok().or_else(|| {
            if let Some(postgres_name) = &service_config.postgres {
                app.postgres_profile(postgres_name)
                    .map(|profile| profile.url.clone())
            } else {
                None
            }
        });

        let session_state_prefix = env::var("SESSION_STATE_PREFIX")
            .ok()
            .or_else(|| service_config.session_state_prefix.clone())
            .unwrap_or_else(|| "storage:session:state".to_string());

        let session_unread_prefix = env::var("SESSION_UNREAD_PREFIX")
            .ok()
            .or_else(|| service_config.session_unread_prefix.clone())
            .unwrap_or_else(|| "storage:session:unread".to_string());

        let user_cursor_prefix = env::var("SESSION_USER_CURSOR_PREFIX")
            .ok()
            .or_else(|| service_config.user_cursor_prefix.clone())
            .unwrap_or_else(|| "storage:user:cursor".to_string());

        let presence_prefix = env::var("SESSION_PRESENCE_PREFIX")
            .ok()
            .or_else(|| service_config.presence_prefix.clone())
            .unwrap_or_else(|| "presence:user".to_string());

        let storage_reader_service = env::var("SESSION_STORAGE_READER_SERVICE")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| service_config.storage_reader_service.clone());

        let recent_message_limit = env::var("SESSION_RECENT_MESSAGE_LIMIT")
            .ok()
            .and_then(|v| v.parse::<i32>().ok())
            .or_else(|| service_config.recent_message_limit)
            .unwrap_or(20);

        // 解析策略配置
        let policy_cfg = service_config.default_policy.as_ref();

        let conflict_resolution = env::var("SESSION_CONFLICT_RESOLUTION")
            .ok()
            .and_then(|s| ConflictResolutionPolicy::from_str(s.trim()))
            .or_else(|| {
                policy_cfg
                    .and_then(|p| p.conflict_resolution.as_ref())
                    .and_then(|s| ConflictResolutionPolicy::from_str(s.trim()))
            })
            .unwrap_or(ConflictResolutionPolicy::Coexist);

        let max_devices = env::var("SESSION_POLICY_MAX_DEVICES")
            .ok()
            .and_then(|v| v.parse::<i32>().ok())
            .filter(|v| *v > 0)
            .or_else(|| policy_cfg.and_then(|p| p.max_devices))
            .filter(|v| *v > 0)
            .unwrap_or(5);

        let allow_anonymous = env::var("SESSION_POLICY_ALLOW_ANONYMOUS")
            .ok()
            .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
            .or_else(|| policy_cfg.and_then(|p| p.allow_anonymous))
            .unwrap_or(false);

        let allow_history_sync = env::var("SESSION_POLICY_ALLOW_HISTORY_SYNC")
            .ok()
            .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
            .or_else(|| policy_cfg.and_then(|p| p.allow_history_sync))
            .unwrap_or(true);

        let mut policy_metadata = HashMap::new();
        if let Ok(raw) = env::var("SESSION_POLICY_METADATA") {
            for kv in raw.split(',') {
                if let Some((k, v)) = kv.split_once('=') {
                    policy_metadata.insert(k.trim().to_string(), v.trim().to_string());
                }
            }
        }

        let default_policy = SessionPolicy {
            conflict_resolution,
            max_devices,
            allow_anonymous,
            allow_history_sync,
            metadata: policy_metadata,
        };

        Ok(Self {
            redis_url,
            postgres_url,
            session_state_prefix,
            session_unread_prefix,
            user_cursor_prefix,
            presence_prefix,
            storage_reader_service,
            recent_message_limit,
            default_policy,
        })
    }
}
