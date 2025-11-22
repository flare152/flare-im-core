//! 推送领域服务
//!
//! 包含推送相关的核心业务逻辑

use std::sync::Arc;

use anyhow::Result;
use flare_proto::access_gateway::PushOptions;
use tracing::instrument;

use crate::domain::model::ConnectionInfo;
use crate::domain::repository::ConnectionQuery;
use crate::infrastructure::online_cache::OnlineStatusCache;
use crate::interface::connection::LongConnectionHandler;

/// 推送结果（领域层）
#[derive(Debug, Clone)]
pub struct DomainPushResult {
    pub user_id: String,
    pub success_count: i32,
    pub failure_count: i32,
    pub error_message: String,
}

/// 推送领域服务
pub struct PushDomainService {
    connection_handler: Arc<LongConnectionHandler>,
    connection_query: Arc<dyn ConnectionQuery>,
    online_cache: Arc<OnlineStatusCache>,
}

impl PushDomainService {
    pub fn new(
        connection_handler: Arc<LongConnectionHandler>,
        connection_query: Arc<dyn ConnectionQuery>,
        online_cache: Arc<OnlineStatusCache>,
    ) -> Self {
        Self {
            connection_handler,
            connection_query,
            online_cache,
        }
    }

    /// 检查用户是否在线（包含缓存逻辑）
    #[instrument(skip(self), fields(user_id = %user_id))]
    pub async fn check_user_online(&self, user_id: &str) -> Result<bool> {
        // 1. 先检查本地缓存
        if let Some((online, _gateway_id)) = self.online_cache.get(user_id).await {
            return Ok(online);
        }

        // 2. 缓存未命中，查询连接管理器
        let connections = self
            .connection_query
            .query_user_connections(user_id)
            .await?;

        let is_online = !connections.is_empty();

        // 3. 更新缓存（即使离线也缓存，避免频繁查询）
        if let Some(gateway_id) = connections.first().map(|_| "local".to_string()) {
            self.online_cache
                .set(user_id.to_string(), gateway_id, is_online)
                .await;
        }

        Ok(is_online)
    }

    /// 过滤连接（根据设备ID和平台）
    pub fn filter_connections(
        &self,
        connections: &[ConnectionInfo],
        options: &PushOptions,
    ) -> Vec<ConnectionInfo> {
        connections
            .iter()
            .filter(|conn| {
                // 过滤设备ID
                if !options.device_ids.is_empty() {
                    if !options.device_ids.contains(&conn.device_id) {
                        return false;
                    }
                }

                // 过滤平台
                if !options.platforms.is_empty() {
                    if !options.platforms.contains(&conn.platform) {
                        return false;
                    }
                }

                true
            })
            .cloned()
            .collect()
    }

    /// 推送消息到连接
    #[instrument(skip(self, message_bytes), fields(user_id = %user_id, connection_count = connections.len()))]
    pub async fn push_to_connections(
        &self,
        user_id: &str,
        connections: &[ConnectionInfo],
        message_bytes: &[u8],
    ) -> Result<(i32, i32)> {
        let mut success_count = 0;
        let mut failure_count = 0;

        for conn in connections {
            match self
                .connection_handler
                .push_message_to_connection(&conn.connection_id, message_bytes.to_vec())
                .await
            {
                Ok(_) => {
                    success_count += 1;
                }
                Err(err) => {
                    failure_count += 1;
                    tracing::warn!(
                        error = %err,
                        user_id = %user_id,
                        connection_id = %conn.connection_id,
                        "Failed to push message to connection"
                    );
                }
            }
        }

        Ok((success_count, failure_count))
    }

    /// 获取用户连接并过滤
    #[instrument(skip(self), fields(user_id = %user_id))]
    pub async fn get_filtered_connections(
        &self,
        user_id: &str,
        options: &PushOptions,
    ) -> Result<Vec<ConnectionInfo>> {
        let connections = self
            .connection_query
            .query_user_connections(user_id)
            .await?;

        Ok(self.filter_connections(&connections, options))
    }

    /// 构建推送结果
    pub fn build_push_result(
        user_id: String,
        success_count: i32,
        failure_count: i32,
    ) -> DomainPushResult {
        DomainPushResult {
            user_id,
            success_count,
            failure_count,
            error_message: if failure_count > 0 {
                format!("Failed to push to {} connections", failure_count)
            } else {
                String::new()
            },
        }
    }
}

