//! 仓储接口（Port）

use async_trait::async_trait;
use flare_server_core::error::Result;

use crate::domain::model::PushDispatchTask;

/// 在线推送发送器（Repository）
///
/// 注意：由于需要作为 trait 对象使用（Arc<dyn OnlinePushSender>），
/// 且方法参数包含引用，Rust 2024 的原生异步 trait 对此有限制，
/// 因此保留 async-trait 宏
#[async_trait]
pub trait OnlinePushSender: Send + Sync {
    async fn send(&self, task: &PushDispatchTask) -> Result<()>;
}

/// 离线推送发送器（Repository）
///
/// 注意：由于需要作为 trait 对象使用，保留 async-trait 宏
#[async_trait]
pub trait OfflinePushSender: Send + Sync {
    async fn send(&self, task: &PushDispatchTask) -> Result<()>;
}

/// ACK 事件
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PushAckEvent {
    pub message_id: String,
    pub user_id: String,
    pub success: bool,
    pub error: Option<String>,
    pub timestamp: i64,
}

/// ACK 发布器（Repository）
///
/// 注意：由于需要作为 trait 对象使用，保留 async-trait 宏
#[async_trait]
pub trait AckPublisher: Send + Sync {
    async fn publish_ack(&self, event: &PushAckEvent) -> Result<()>;
}

/// 死信队列发布器（Repository）
///
/// 注意：由于需要作为 trait 对象使用，保留 async-trait 宏
#[async_trait]
pub trait DlqPublisher: Send + Sync {
    async fn publish_to_dlq(&self, task: &PushDispatchTask, error: &str) -> Result<()>;
}
