//! 消息状态跟踪（全链路消息状态跟踪）

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{debug, info};

use crate::config::PushServerConfig;

/// 消息状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageStatus {
    /// 待处理（已入Kafka队列）
    Pending,
    /// 推送中
    Pushing,
    /// 已推送（等待ACK）
    Pushed,
    /// 已送达（收到客户端ACK）
    Delivered,
    /// 推送失败
    Failed,
    /// 重试中
    Retrying,
    /// 死信队列
    Dlq,
    /// 已过期（通知消息离线舍弃）
    Expired,
}

/// 消息状态记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageState {
    pub message_id: String,
    pub user_id: String,
    pub status: MessageStatus,
    pub message_type: String, // "Normal" | "Notification"
    pub push_attempts: u32,
    pub last_push_at: Option<DateTime<Utc>>,
    pub ack_received_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 推送统计信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushStatistics {
    /// 总推送数
    pub total_pushes: u64,
    /// 成功率
    pub success_rate: f64,
    /// 待处理数
    pub pending_count: u64,
    /// 已送达数
    pub delivered_count: u64,
    /// 失败数
    pub failed_count: u64,
    /// 平均推送时间（毫秒）
    pub average_delivery_time_ms: f64,
}

impl Default for PushStatistics {
    fn default() -> Self {
        Self {
            total_pushes: 0,
            success_rate: 0.0,
            pending_count: 0,
            delivered_count: 0,
            failed_count: 0,
            average_delivery_time_ms: 0.0,
        }
    }
}

/// 消息状态跟踪器
pub struct MessageStateTracker {
    #[allow(dead_code)]
    config: Arc<PushServerConfig>,
    /// 内存状态缓存（message_id:user_id -> MessageState）
    state_cache: Arc<RwLock<HashMap<String, MessageState>>>,
    /// Redis客户端（用于持久化）
    redis_client: Option<Arc<redis::Client>>,
}

impl MessageStateTracker {
    pub fn new(
        config: Arc<PushServerConfig>,
        redis_client: Option<Arc<redis::Client>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            config,
            state_cache: Arc::new(RwLock::new(HashMap::new())),
            redis_client,
        })
    }

    /// 更新消息状态
    pub async fn update_status(
        &self,
        message_id: &str,
        user_id: &str,
        status: MessageStatus,
        error: Option<String>,
    ) {
        let key = format!("{}:{}", message_id, user_id);
        let now = Utc::now();

        let mut cache = self.state_cache.write().await;
        let state = cache.entry(key.clone()).or_insert_with(|| MessageState {
            message_id: message_id.to_string(),
            user_id: user_id.to_string(),
            status: MessageStatus::Pending,
            message_type: String::new(),
            push_attempts: 0,
            last_push_at: None,
            ack_received_at: None,
            error: None,
            created_at: now,
            updated_at: now,
        });

        state.status = status;
        state.updated_at = now;
        if let Some(err) = error {
            state.error = Some(err);
        }

        if status == MessageStatus::Pushing {
            state.push_attempts += 1;
            state.last_push_at = Some(now);
        }

        if status == MessageStatus::Delivered {
            state.ack_received_at = Some(now);
        }

        // 持久化到Redis
        if let Some(_redis) = &self.redis_client {
            if let Ok(_state_json) = serde_json::to_string(state) {
                let _state_key = format!("msg:state:{}:{}", message_id, user_id);
                // 注意：这里需要异步Redis操作，简化实现
                // 实际应该使用异步Redis客户端
                debug!(
                    message_id = %message_id,
                    user_id = %user_id,
                    status = ?status,
                    "Updated message state"
                );
            }
        }

        info!(
            message_id = %message_id,
            user_id = %user_id,
            status = ?status,
            push_attempts = state.push_attempts,
            "Message state updated"
        );
    }

    /// 获取消息状态
    pub async fn get_status(&self, message_id: &str, user_id: &str) -> Option<MessageState> {
        let key = format!("{}:{}", message_id, user_id);
        let cache = self.state_cache.read().await;
        cache.get(&key).cloned()
    }

    /// 设置消息类型
    pub async fn set_message_type(&self, message_id: &str, user_id: &str, message_type: String) {
        let key = format!("{}:{}", message_id, user_id);
        let mut cache = self.state_cache.write().await;
        if let Some(state) = cache.get_mut(&key) {
            state.message_type = message_type;
        }
    }

    /// 获取推送统计信息
    pub async fn get_statistics(&self) -> PushStatistics {
        let cache = self.state_cache.read().await;

        let mut stats = PushStatistics::default();
        let mut total_delivery_time_ms = 0u64;
        let mut delivered_with_time_count = 0u64;

        for state in cache.values() {
            stats.total_pushes += 1;

            match state.status {
                MessageStatus::Pending => stats.pending_count += 1,
                MessageStatus::Delivered => {
                    stats.delivered_count += 1;
                    // 计算平均推送时间
                    let created = state.created_at.timestamp_millis();
                    if let Some(delivered) = state.ack_received_at.map(|dt| dt.timestamp_millis()) {
                        let delivery_time = delivered.saturating_sub(created);
                        total_delivery_time_ms =
                            total_delivery_time_ms.saturating_add(delivery_time as u64);
                        delivered_with_time_count += 1;
                    }
                }
                MessageStatus::Failed => stats.failed_count += 1,
                _ => {}
            }
        }

        // 计算成功率
        if stats.total_pushes > 0 {
            stats.success_rate = (stats.delivered_count as f64) / (stats.total_pushes as f64);
        }

        // 计算平均推送时间
        if delivered_with_time_count > 0 {
            stats.average_delivery_time_ms =
                (total_delivery_time_ms as f64) / (delivered_with_time_count as f64);
        }

        stats
    }
}
