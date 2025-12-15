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
    /// 
    /// 优化：去重连接，避免重复推送
    #[instrument(skip(self, message_bytes), fields(user_id = %user_id, connection_count = connections.len()))]
    pub async fn push_to_connections(
        &self,
        user_id: &str,
        connections: &[ConnectionInfo],
        message_bytes: &[u8],
    ) -> Result<(i32, i32)> {
        let start_time = std::time::Instant::now();
        
        // 去重连接：使用 HashSet 去重 connection_id，避免重复推送
        use std::collections::HashSet;
        let mut seen_connection_ids = HashSet::new();
        let mut unique_connections = Vec::new();
        
        for conn in connections {
            if seen_connection_ids.insert(conn.connection_id.clone()) {
                unique_connections.push(conn);
            } else {
                tracing::warn!(
                    user_id = %user_id,
                    connection_id = %conn.connection_id,
                    timestamp_ms = start_time.elapsed().as_millis(),
                    "Duplicate connection detected, skipping"
                );
            }
        }
        
        let dedup_duration_ms = start_time.elapsed().as_millis();
        tracing::debug!(
            user_id = %user_id,
            original_count = connections.len(),
            unique_count = unique_connections.len(),
            dedup_duration_ms = dedup_duration_ms,
            "Connection deduplication completed"
        );
        
        let mut success_count = 0;
        let mut failure_count = 0;
        let push_start = std::time::Instant::now();
        for conn in &unique_connections {
            let conn_start = std::time::Instant::now();
            match self
                .connection_handler
                .push_message_to_connection(&conn.connection_id, message_bytes.to_vec())
                .await
            {
                Ok(_) => {
                    success_count += 1;
                    tracing::debug!(
                        user_id = %user_id,
                        connection_id = %conn.connection_id,
                        push_duration_ms = conn_start.elapsed().as_millis(),
                        "Message pushed to connection successfully"
                    );
                }
                Err(err) => {
                    failure_count += 1;
                    tracing::warn!(
                        error = %err,
                        user_id = %user_id,
                        connection_id = %conn.connection_id,
                        push_duration_ms = conn_start.elapsed().as_millis(),
                        "Failed to push message to connection"
                    );
                }
            }
        }
        
        let total_duration_ms = start_time.elapsed().as_millis();
        let push_duration_ms = push_start.elapsed().as_millis();
        tracing::debug!(
            user_id = %user_id,
            success_count = success_count,
            failure_count = failure_count,
            total_duration_ms = total_duration_ms,
            push_duration_ms = push_duration_ms,
            "Push to connections completed"
        );

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

    /// 推送 ACK 数据包给用户
    /// 
    /// 策略：
    /// 1. ACK 推送给用户的所有在线设备
    /// 2. 推送失败不影响其他用户
    /// 3. 记录推送结果用于统计
    #[instrument(skip(self), fields(user_id = %user_id))]
    pub async fn push_ack_to_user(
        &self,
        user_id: &str,
        packet: flare_proto::common::ServerPacket,
    ) -> Result<()> {
        // 查询用户的所有连接
        let connections = self
            .connection_query
            .query_user_connections(user_id)
            .await?;

        if connections.is_empty() {
            tracing::debug!(user_id = %user_id, "No online connections for ACK push");
            return Ok(());
        }

        let mut success_count = 0;
        let mut failure_count = 0;

        // 向每个连接发送 ACK 数据包
        for conn in &connections {
            match self.connection_handler.push_packet_to_connection(&conn.connection_id, &packet).await {
                Ok(_) => {
                    success_count += 1;
                    tracing::debug!(
                        user_id = %user_id,
                        connection_id = %conn.connection_id,
                        "ACK packet pushed successfully"
                    );
                }
                Err(err) => {
                    failure_count += 1;
                    tracing::warn!(
                        error = %err,
                        user_id = %user_id,
                        connection_id = %conn.connection_id,
                        "Failed to push ACK packet"
                    );
                }
            }
        }

        if failure_count > 0 {
            tracing::warn!(
                user_id = %user_id,
                success_count = success_count,
                failure_count = failure_count,
                "ACK push completed with some failures"
            );
        } else {
            tracing::info!(
                user_id = %user_id,
                success_count = success_count,
                "ACK push completed successfully"
            );
        }

        Ok(())
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
