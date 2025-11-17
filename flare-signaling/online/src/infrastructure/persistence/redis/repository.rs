use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use flare_server_core::error::{ErrorBuilder, ErrorCode, Result};
use redis::{AsyncCommands, aio::ConnectionManager};
use serde_json::json;

use crate::config::OnlineConfig;
use crate::domain::entities::{OnlineStatusRecord, SessionRecord};
use crate::domain::repositories::SessionRepository;

const SESSION_KEY_PREFIX: &str = "session";

pub struct RedisSessionRepository {
    client: Arc<redis::Client>,
    config: Arc<OnlineConfig>,
}

impl RedisSessionRepository {
    pub fn new(client: Arc<redis::Client>, config: Arc<OnlineConfig>) -> Self {
        Self { client, config }
    }

    fn session_key(&self, user_id: &str) -> String {
        format!("{}:{}", SESSION_KEY_PREFIX, user_id)
    }

    async fn connection(&self) -> Result<ConnectionManager> {
        ConnectionManager::new(self.client.as_ref().clone())
            .await
            .map_err(|err| {
                ErrorBuilder::new(
                    ErrorCode::ServiceUnavailable,
                    "failed to open redis connection",
                )
                .details(err.to_string())
                .build_error()
            })
    }

    fn to_timestamp(seconds: i64) -> Option<DateTime<Utc>> {
        Utc.timestamp_opt(seconds, 0).single()
    }
}

#[async_trait]
impl SessionRepository for RedisSessionRepository {
    async fn save_session(&self, record: &SessionRecord) -> Result<()> {
        let mut conn = self.connection().await?;
        let key = self.session_key(&record.user_id);
        let value = json!({
            "session_id": record.session_id,
            "gateway_id": record.gateway_id,
            "server_id": record.server_id,
            "device_id": record.device_id,
            "device_platform": record.device_platform,
            "last_seen": record.last_seen.timestamp(),
        });
        let _: () = conn.set(&key, value.to_string()).await.map_err(|err| {
            ErrorBuilder::new(ErrorCode::DatabaseError, "failed to store session")
                .details(err.to_string())
                .build_error()
        })?;
        let _: bool = conn
            .expire(&key, self.config.redis_ttl_seconds as i64)
            .await
            .map_err(|err| {
                ErrorBuilder::new(ErrorCode::DatabaseError, "failed to set session ttl")
                    .details(err.to_string())
                    .build_error()
            })?;
        Ok(())
    }

    async fn remove_session(&self, session_id: &str, user_id: &str) -> Result<()> {
        let mut conn = self.connection().await?;
        let key = self.session_key(user_id);
        let _: usize = conn.del(&key).await.map_err(|err| {
            ErrorBuilder::new(ErrorCode::DatabaseError, "failed to delete session")
                .details(err.to_string())
                .build_error()
        })?;
        tracing::info!(%session_id, %user_id, "session removed from redis");
        Ok(())
    }

    async fn touch_session(&self, user_id: &str) -> Result<()> {
        let mut conn = self.connection().await?;
        let key = self.session_key(user_id);
        let _: bool = conn
            .expire(&key, self.config.redis_ttl_seconds as i64)
            .await
            .map_err(|err| {
                ErrorBuilder::new(ErrorCode::DatabaseError, "failed to refresh session ttl")
                    .details(err.to_string())
                    .build_error()
            })?;
        Ok(())
    }

    async fn fetch_statuses(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, OnlineStatusRecord>> {
        let mut conn = self.connection().await?;
        let mut result = HashMap::new();
        for user_id in user_ids {
            let key = self.session_key(user_id);
            let value: Option<String> = conn.get(&key).await.map_err(|err| {
                ErrorBuilder::new(ErrorCode::DatabaseError, "failed to read session")
                    .details(err.to_string())
                    .build_error()
            })?;
            if let Some(payload) = value {
                let json: serde_json::Value = serde_json::from_str(&payload).map_err(|err| {
                    ErrorBuilder::new(
                        ErrorCode::DeserializationError,
                        "failed to decode session json",
                    )
                    .details(err.to_string())
                    .build_error()
                })?;
                let last_seen = json
                    .get("last_seen")
                    .and_then(|v| v.as_i64())
                    .and_then(Self::to_timestamp);
                result.insert(
                    user_id.clone(),
                    OnlineStatusRecord {
                        online: true,
                        server_id: json
                            .get("server_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        gateway_id: json
                            .get("gateway_id")
                            .and_then(|v| v.as_str())
                            .map(|v| v.to_string()),
                        cluster_id: None,
                        last_seen,
                        device_id: json
                            .get("device_id")
                            .and_then(|v| v.as_str())
                            .map(|v| v.to_string()),
                        device_platform: json
                            .get("device_platform")
                            .and_then(|v| v.as_str())
                            .map(|v| v.to_string()),
                    },
                );
            }
        }

        Ok(result)
    }

    async fn get_user_sessions(&self, user_id: &str) -> Result<Vec<SessionRecord>> {
        let mut conn = self.connection().await?;
        let key = self.session_key(user_id);
        let value: Option<String> = conn.get(&key).await.map_err(|err| {
            ErrorBuilder::new(ErrorCode::DatabaseError, "failed to read session")
                .details(err.to_string())
                .build_error()
        })?;
        
        if let Some(payload) = value {
            let json: serde_json::Value = serde_json::from_str(&payload).map_err(|err| {
                ErrorBuilder::new(
                    ErrorCode::DeserializationError,
                    "failed to decode session json",
                )
                .details(err.to_string())
                .build_error()
            })?;
            
            let last_seen = json
                .get("last_seen")
                .and_then(|v| v.as_i64())
                .and_then(Self::to_timestamp)
                .unwrap_or_else(Utc::now);
            
            Ok(vec![SessionRecord {
                session_id: json
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                user_id: user_id.to_string(),
                device_id: json
                    .get("device_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                device_platform: json
                    .get("device_platform")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                server_id: json
                    .get("server_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                gateway_id: json
                    .get("gateway_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                last_seen,
            }])
        } else {
            Ok(vec![])
        }
    }

    async fn remove_user_sessions(&self, user_id: &str, device_ids: Option<&[String]>) -> Result<()> {
        let mut conn = self.connection().await?;
        let key = self.session_key(user_id);
        
        // 如果指定了设备ID列表，需要检查设备是否匹配
        // 当前实现中，一个用户只有一个会话，所以直接删除
        // 未来如果需要支持多设备，可以扩展为Hash结构存储多个设备会话
        if let Some(device_ids) = device_ids {
            // 获取当前会话
            let value: Option<String> = conn.get(&key).await.map_err(|err| {
                ErrorBuilder::new(ErrorCode::DatabaseError, "failed to read session")
                    .details(err.to_string())
                    .build_error()
            })?;
            
            if let Some(payload) = value {
                let json: serde_json::Value = serde_json::from_str(&payload).map_err(|err| {
                    ErrorBuilder::new(
                        ErrorCode::DeserializationError,
                        "failed to decode session json",
                    )
                    .details(err.to_string())
                    .build_error()
                })?;
                
                let current_device_id = json
                    .get("device_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                
                // 只删除匹配的设备
                if device_ids.contains(&current_device_id.to_string()) {
                    let _: usize = conn.del(&key).await.map_err(|err| {
                        ErrorBuilder::new(ErrorCode::DatabaseError, "failed to delete session")
                            .details(err.to_string())
                            .build_error()
                    })?;
                }
            }
        } else {
            // 删除所有会话
            let _: usize = conn.del(&key).await.map_err(|err| {
                ErrorBuilder::new(ErrorCode::DatabaseError, "failed to delete session")
                    .details(err.to_string())
                    .build_error()
            })?;
        }
        
        Ok(())
    }

    async fn get_session_by_device(&self, user_id: &str, device_id: &str) -> Result<Option<SessionRecord>> {
        let sessions = self.get_user_sessions(user_id).await?;
        Ok(sessions.into_iter().find(|s| s.device_id == device_id))
    }

    async fn list_user_devices(&self, user_id: &str) -> Result<Vec<crate::domain::entities::DeviceInfo>> {
        let sessions = self.get_user_sessions(user_id).await?;
        let devices: Vec<crate::domain::entities::DeviceInfo> = sessions
            .into_iter()
            .map(|s| crate::domain::entities::DeviceInfo {
                device_id: s.device_id,
                platform: s.device_platform,
                model: None,
                os_version: None,
                last_active_time: s.last_seen,
            })
            .collect();
        Ok(devices)
    }

    async fn get_device(&self, user_id: &str, device_id: &str) -> Result<Option<crate::domain::entities::DeviceInfo>> {
        let session = self.get_session_by_device(user_id, device_id).await?;
        Ok(session.map(|s| crate::domain::entities::DeviceInfo {
            device_id: s.device_id,
            platform: s.device_platform,
            model: None,
            os_version: None,
            last_active_time: s.last_seen,
        }))
    }
}
