//! ACK确认机制（多级ACK跟踪）

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::config::PushServerConfig;

/// ACK类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckType {
    /// 客户端ACK
    ClientAck,
    /// 推送ACK
    PushAck,
    /// 存储ACK
    StorageAck,
}

/// ACK状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckStatus {
    /// 待确认
    Pending,
    /// 已确认
    Confirmed,
    /// 超时
    Timeout,
    /// 失败
    Failed,
}

/// 待确认的ACK
#[derive(Debug, Clone)]
struct PendingAck {
    message_id: String,
    user_id: String,
    ack_type: AckType,
    created_at: DateTime<Utc>,
    timeout_at: DateTime<Utc>,
}

/// ACK跟踪器
pub struct AckTracker {
    config: Arc<PushServerConfig>,
    /// 待确认的ACK（message_id:user_id -> PendingAck）
    pending_acks: Arc<RwLock<HashMap<String, PendingAck>>>,
}

impl AckTracker {
    pub fn new(config: Arc<PushServerConfig>) -> Arc<Self> {
        Arc::new(Self {
            config,
            pending_acks: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// 注册待确认的ACK
    pub async fn register_pending_ack(
        &self,
        message_id: &str,
        user_id: &str,
        ack_type: AckType,
    ) {
        let key = format!("{}:{}", message_id, user_id);
        let timeout_at = Utc::now()
            + chrono::Duration::seconds(self.config.ack_timeout_seconds as i64);

        let pending_ack = PendingAck {
            message_id: message_id.to_string(),
            user_id: user_id.to_string(),
            ack_type,
            created_at: Utc::now(),
            timeout_at,
        };

        let mut acks = self.pending_acks.write().await;
        acks.insert(key, pending_ack);

        debug!(
            message_id = %message_id,
            user_id = %user_id,
            ack_type = ?ack_type,
            timeout_seconds = self.config.ack_timeout_seconds,
            "Registered pending ACK"
        );
    }

    /// 确认ACK
    pub async fn confirm_ack(
        &self,
        message_id: &str,
        user_id: &str,
        ack_type: AckType,
    ) -> bool {
        let key = format!("{}:{}", message_id, user_id);
        let mut acks = self.pending_acks.write().await;

        if let Some(pending) = acks.remove(&key) {
            if pending.ack_type == ack_type {
                info!(
                    message_id = %message_id,
                    user_id = %user_id,
                    ack_type = ?ack_type,
                    duration_ms = (Utc::now() - pending.created_at).num_milliseconds(),
                    "ACK confirmed"
                );
                return true;
            } else {
                // ACK类型不匹配，重新插入
                acks.insert(key, pending);
            }
        }

        false
    }

    /// 检查并清理超时的ACK
    pub async fn check_timeout(&self) -> Vec<(String, String, AckType)> {
        let now = Utc::now();
        let mut timeout_acks = Vec::new();
        let mut acks = self.pending_acks.write().await;

        let keys_to_remove: Vec<String> = acks
            .iter()
            .filter_map(|(key, pending)| {
                if pending.timeout_at < now {
                    Some((key.clone(), pending.clone()))
                } else {
                    None
                }
            })
            .map(|(key, pending)| {
                timeout_acks.push((
                    pending.message_id.clone(),
                    pending.user_id.clone(),
                    pending.ack_type,
                ));
                key
            })
            .collect();

        for key in keys_to_remove {
            acks.remove(&key);
        }

        if !timeout_acks.is_empty() {
            warn!(
                count = timeout_acks.len(),
                "Found timeout ACKs"
            );
        }

        timeout_acks
    }

    /// 启动ACK监控任务
    pub fn start_monitor(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(
                self.config.ack_monitor_interval_seconds,
            ));

            loop {
                interval.tick().await;
                let timeout_acks = self.check_timeout().await;

                // 处理超时的ACK（触发重试或降级）
                for (message_id, user_id, ack_type) in timeout_acks {
                    warn!(
                        message_id = %message_id,
                        user_id = %user_id,
                        ack_type = ?ack_type,
                        "ACK timeout, may need retry or fallback"
                    );
                    // TODO: 触发重试或降级到离线推送
                }
            }
        })
    }
}

