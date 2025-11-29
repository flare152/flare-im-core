//! 连接查询实现
//!
//! 基于 ConnectionManager 实现连接查询

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use flare_core::server::connection::ConnectionManagerTrait;
use flare_server_core::error::Result;

use crate::domain::repository::ConnectionQuery;
use crate::domain::model::ConnectionInfo;

/// 基于 ConnectionManager 的连接查询实现
pub struct ManagerConnectionQuery {
    connection_manager: Arc<dyn ConnectionManagerTrait>,
}

impl ManagerConnectionQuery {
    pub fn new(connection_manager: Arc<dyn ConnectionManagerTrait>) -> Self {
        Self {
            connection_manager,
        }
    }
}


#[async_trait]
impl ConnectionQuery for ManagerConnectionQuery {
    async fn query_user_connections(&self, user_id: &str) -> Result<Vec<ConnectionInfo>> {
        // 获取用户的所有连接ID
        let connection_ids = self.connection_manager.get_user_connections(user_id).await;

        let mut connections = Vec::new();

        // 获取每个连接的详细信息
        for connection_id in connection_ids {
            if let Some((_, conn_info)) = self
                .connection_manager
                .get_connection(&connection_id)
                .await
            {
                // 获取设备信息
                let device_id = conn_info
                    .device_info
                    .as_ref()
                    .map(|d| d.device_id.clone())
                    .unwrap_or_else(|| "unknown".to_string());

                let platform = conn_info
                    .device_info
                    .as_ref()
                    .map(|d| format!("{:?}", d.platform))
                    .unwrap_or_else(|| "unknown".to_string());

                // 确定协议类型（从 metadata 或默认）
                let protocol = conn_info
                    .metadata
                    .get("protocol")
                    .cloned()
                    .unwrap_or_else(|| "websocket".to_string());

                // 转换时间戳
                let connected_at = if conn_info.created_at > 0 {
                    Some(
                        DateTime::from_timestamp(conn_info.created_at as i64, 0)
                            .unwrap_or_else(Utc::now),
                    )
                } else {
                    None
                };

                let last_active_at = if conn_info.last_active > 0 {
                    Some(
                        DateTime::from_timestamp(conn_info.last_active as i64, 0)
                            .unwrap_or_else(Utc::now),
                    )
                } else {
                    None
                };

                connections.push(ConnectionInfo {
                    connection_id: conn_info.connection_id,
                    protocol,
                    device_id,
                    platform,
                    connected_at,
                    last_active_at,
                });
            }
        }

        Ok(connections)
    }
}

