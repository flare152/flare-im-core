use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use flare_server_core::error::{ErrorBuilder, ErrorCode, Result};
use redis::aio::ConnectionManager;
use serde_json::json;
use tracing::{debug, warn};

use crate::domain::repositories::{OnlineStatus, OnlineStatusRepository};
use crate::infrastructure::signaling::SignalingOnlineClient;

pub struct RedisOnlineStatusRepository {
    client: Arc<redis::Client>,
    ttl_seconds: u64,
    signaling_client: Arc<SignalingOnlineClient>,
    default_tenant_id: String,
}

impl RedisOnlineStatusRepository {
    pub fn new(
        client: Arc<redis::Client>,
        ttl_seconds: u64,
        signaling_client: Arc<SignalingOnlineClient>,
        default_tenant_id: String,
    ) -> Self {
        Self {
            client,
            ttl_seconds,
            signaling_client,
            default_tenant_id,
        }
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

    #[allow(dead_code)]
    fn key(user_id: &str) -> String {
        format!("push:online:{}", user_id)
    }
}

#[async_trait]
impl OnlineStatusRepository for RedisOnlineStatusRepository {
    async fn is_online(&self, user_id: &str) -> Result<bool> {
        // 单个查询也使用批量查询接口
        let statuses = self
            .batch_get_online_status(&[user_id.to_string()])
            .await?;
        
        Ok(statuses
            .get(user_id)
            .map(|s| s.online)
            .unwrap_or(false))
    }
    
    async fn batch_get_online_status(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, OnlineStatus>> {
        // 通过Signaling Online服务批量查询
        self.signaling_client
            .batch_get_online_status(user_ids, Some(&self.default_tenant_id))
            .await
    }
    
    async fn get_all_online_users(
        &self,
        tenant_id: Option<&str>,
    ) -> Result<HashMap<String, OnlineStatus>> {
        // 通过Signaling Online服务查询所有在线用户
        // 注意：目前 Signaling Online 可能不支持查询所有在线用户
        // 这里先通过 Redis 扫描所有在线用户 key
        let _tenant = tenant_id.unwrap_or(&self.default_tenant_id);
        let mut conn = self.connection().await?;
        
        // Redis key 格式: session:{user_id} (根据 Signaling Online 的 RedisSessionRepository)
        let pattern = "session:*";
        let mut online_users = HashMap::new();
        
        debug!(
            tenant_id = %tenant_id.unwrap_or(&self.default_tenant_id),
            "Scanning Redis for online users with pattern: {}",
            pattern
        );
        
        // 使用 SCAN 扫描所有匹配的 key
        let mut cursor: isize = 0;
        loop {
            let result: (isize, Vec<String>) = redis::cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg(&pattern)
                .arg("COUNT")
                .arg(100)
                .query_async(&mut conn)
                .await
                .map_err(|err| {
                    ErrorBuilder::new(
                        ErrorCode::ServiceUnavailable,
                        "failed to scan online users",
                    )
                    .details(err.to_string())
                    .build_error()
                })?;
            
            cursor = result.0;
            let keys = result.1;
            
            debug!(
                keys_count = keys.len(),
                cursor = cursor,
                "Scanned {} keys from Redis",
                keys.len()
            );
            
            // 从 key 中提取 user_id，并查询在线状态
            for key in keys {
                // key 格式: session:{user_id}
                if let Some(user_id) = key.strip_prefix("session:") {
                    match self.batch_get_online_status(&[user_id.to_string()]).await {
                        Ok(statuses) => {
                            if let Some(status) = statuses.get(user_id) {
                                if status.online {
                                    online_users.insert(user_id.to_string(), status.clone());
                                } else {
                                    debug!(
                                        user_id = %user_id,
                                        "User found in Redis but not online according to Signaling Online"
                                    );
                                }
                            } else {
                                warn!(
                                    user_id = %user_id,
                                    "User found in Redis but not found in Signaling Online response"
                                );
                            }
                        }
                        Err(err) => {
                            warn!(
                                error = %err,
                                user_id = %user_id,
                                "Failed to query online status from Signaling Online"
                            );
                        }
                    }
                }
            }
            
            if cursor == 0 {
                break;
            }
        }
        
        debug!(
            total_online_users = online_users.len(),
            "Finished scanning Redis, found {} online users",
            online_users.len()
        );
        
        Ok(online_users)
    }
}
