use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::{DateTime, TimeZone, Utc};
use redis::{AsyncCommands, aio::ConnectionManager};
use serde_json::json;

use crate::config::OnlineConfig;
use crate::domain::aggregate::Session;
use crate::domain::value_object::{SessionId, UserId, DeviceId, DevicePriority, TokenVersion, ConnectionQuality};
use crate::domain::model::OnlineStatusRecord;
use crate::domain::repository::SessionRepository;
use async_trait::async_trait;

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
            .context("failed to open redis connection")
    }

    fn to_timestamp(seconds: i64) -> Option<DateTime<Utc>> {
        Utc.timestamp_opt(seconds, 0).single()
    }
}


#[async_trait]
impl SessionRepository for RedisSessionRepository {
    async fn save_session(&self, session: &Session) -> Result<()> {
        let mut conn = self.connection().await?;
        let key = self.session_key(session.user_id().as_str());
        let value = json!({
            "session_id": session.id().as_str(),
            "gateway_id": session.gateway_id(),
            "server_id": session.server_id(),
            "device_id": session.device_id().as_str(),
            "device_platform": session.device_platform(),
            "last_seen": session.last_heartbeat_at().timestamp(),
            "device_priority": session.device_priority().as_i32(),
            "token_version": session.token_version().value(),
        });
        let _: () = conn.set(&key, value.to_string())
            .await
            .context("failed to store session")?;
        let _: bool = conn.expire(&key, self.config.redis_ttl_seconds as i64)
            .await
            .context("failed to set session ttl")?;
        Ok(())
    }

    async fn remove_session(&self, session_id: &SessionId, user_id: &UserId) -> Result<()> {
        let mut conn = self.connection().await?;
        let key = self.session_key(user_id.as_str());
        let _: usize = conn.del(&key)
            .await
            .context("failed to delete session")?;
        tracing::info!(session_id = %session_id.as_ref(), user_id = %user_id.as_ref(), "session removed from redis");
        Ok(())
    }

    async fn touch_session(&self, user_id: &UserId) -> Result<()> {
        let mut conn = self.connection().await?;
        let key = self.session_key(user_id.as_str());
        let _: bool = conn.expire(&key, self.config.redis_ttl_seconds as i64)
            .await
            .context("failed to refresh session ttl")?;
        Ok(())
    }

    async fn fetch_statuses(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, OnlineStatusRecord>> {
        let mut conn = self.connection().await?;
        let mut result = HashMap::new();
        for user_id in user_ids {
            let key = self.session_key(user_id.as_str());
            let value: Option<String> = conn.get(&key)
                .await
                .context("failed to read session")?;
            if let Some(payload) = value {
                let json: serde_json::Value = serde_json::from_str(&payload)
                    .context("failed to decode session json")?;
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

    async fn get_user_sessions(&self, user_id: &UserId) -> Result<Vec<Session>> {
        let mut conn = self.connection().await?;
        let key = self.session_key(user_id.as_str());
        let value: Option<String> = conn.get(&key)
            .await
            .context("failed to read session")?;
        
        if let Some(payload) = value {
            let json: serde_json::Value = serde_json::from_str(&payload)
                .context("failed to decode session json")?;
            
            let session_id_str = json
                .get("session_id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let session_id = SessionId::from_string(session_id_str)
                .map_err(|e| anyhow::anyhow!(e))?;
            
            let device_id_str = json
                .get("device_id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let device_id = DeviceId::new(device_id_str)
                .map_err(|e| anyhow::anyhow!(e))?;
            
            let device_platform = json
                .get("device_platform")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            
            let server_id = json
                .get("server_id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            
            let gateway_id = json
                .get("gateway_id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            
            let last_seen = json
                .get("last_seen")
                .and_then(|v| v.as_i64())
                .and_then(Self::to_timestamp)
                .unwrap_or_else(Utc::now);
            
            let created_at = last_seen;
            
            let device_priority_i32 = json
                .get("device_priority")
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i32;
            let device_priority = DevicePriority::from_i32(device_priority_i32);
            
            let token_version_i64 = json
                .get("token_version")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let token_version = TokenVersion::from(token_version_i64);
            
            let connection_quality: Option<ConnectionQuality> = None;
            
            let session = Session::reconstitute(
                session_id,
                user_id.clone(),
                device_id,
                device_platform,
                server_id,
                gateway_id,
                device_priority,
                token_version,
                connection_quality,
                created_at,
                last_seen,
            );
            
            Ok(vec![session])
        } else {
            Ok(vec![])
        }
    }

    async fn remove_user_sessions(&self, user_id: &UserId, device_ids: Option<&[DeviceId]>) -> Result<()> {
        let mut conn = self.connection().await?;
        let key = self.session_key(user_id.as_str());
        
        // 如果指定了设备ID列表，需要检查设备是否匹配
        // 当前实现中，一个用户只有一个会话，所以直接删除
        // 未来如果需要支持多设备，可以扩展为Hash结构存储多个设备会话
        if let Some(device_ids) = device_ids {
            // 获取当前会话
            let value: Option<String> = conn.get(&key)
                .await
                .context("failed to read session")?;
            
            if let Some(payload) = value {
                let json: serde_json::Value = serde_json::from_str(&payload)
                    .context("failed to decode session json")?;
                
                let current_device_id = json
                    .get("device_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                
                // 只删除匹配的设备
                if device_ids.iter().any(|d| d.as_str() == current_device_id) {
                    let _: usize = conn.del(&key)
                        .await
                        .context("failed to delete session")?;
                }
            }
        } else {
            // 删除所有会话
            let _: usize = conn.del(&key)
                .await
                .context("failed to delete session")?;
        }
        
        Ok(())
    }

    async fn get_session_by_device(&self, user_id: &UserId, device_id: &DeviceId) -> Result<Option<Session>> {
        let sessions = self.get_user_sessions(user_id).await?;
        Ok(sessions.into_iter().find(|s| s.device_id().as_str() == device_id.as_str()))
    }

    async fn list_user_devices(&self, user_id: &str) -> Result<Vec<crate::domain::model::DeviceInfo>> {
        let sessions = self.get_user_sessions(&UserId::new(user_id.to_string()).unwrap()).await?;
        let devices: Vec<crate::domain::model::DeviceInfo> = sessions
            .into_iter()
            .map(|s| crate::domain::model::DeviceInfo {
                device_id: s.device_id().as_str().to_string(),
                platform: s.device_platform().to_string(),
                model: None,
                os_version: None,
                last_active_time: s.last_heartbeat_at(),
            })
            .collect();
        Ok(devices)
    }

    async fn get_device(&self, user_id: &str, device_id: &str) -> Result<Option<crate::domain::model::DeviceInfo>> {
        let session = self.get_session_by_device(&UserId::new(user_id.to_string()).unwrap(), &DeviceId::new(device_id.to_string()).unwrap()).await?;
        Ok(session.map(|s| crate::domain::model::DeviceInfo {
            device_id: s.device_id().as_str().to_string(),
            platform: s.device_platform().to_string(),
            model: None,
            os_version: None,
            last_active_time: s.last_heartbeat_at(),
        }))
    }
}
