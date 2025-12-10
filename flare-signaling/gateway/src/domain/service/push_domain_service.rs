//! 推送领域服务
//!
//! 包含推送相关的核心业务逻辑

use std::sync::Arc;

use anyhow::Result;
use flare_proto::access_gateway::PushOptions;
use tracing::instrument;

use crate::domain::model::ConnectionInfo;
use crate::domain::repository::ConnectionQuery;
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
}

impl PushDomainService {
    pub fn new(
        connection_handler: Arc<LongConnectionHandler>,
        connection_query: Arc<dyn ConnectionQuery>,
    ) -> Self {
        Self {
            connection_handler,
            connection_query,
        }
    }

    /// 检查用户是否在线
    /// 
    /// Gateway 直接查询本地连接状态，不维护缓存
    /// 在线状态由 Signaling Online 服务统一管理
    #[instrument(skip(self), fields(user_id = %user_id))]
    pub async fn check_user_online(&self, user_id: &str) -> Result<bool> {
        // 直接查询本地连接状态
        let connections = self
            .connection_query
            .query_user_connections(user_id)
            .await?;

        Ok(!connections.is_empty())
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

    /// 推送消息到连接（直接单条推送，保持 Gateway 轻量）
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
