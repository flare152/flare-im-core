//! 基础设施层（Repository impl）

pub mod ack_publisher;
pub mod dlq_publisher;
pub mod hook;
pub mod offline;
pub mod online;
pub mod retry;

pub use ack_publisher::{KafkaAckPublisher, NoopAckPublisher};
pub use dlq_publisher::KafkaDlqPublisher;
pub use offline::{build_offline_sender, OfflinePushSenderRef, NoopOfflinePushSender};
pub use online::{build_online_sender, OnlinePushSenderRef, NoopOnlinePushSender};
pub use retry::{execute_with_retry, RetryPolicy, RetryableError};
