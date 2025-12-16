//! 基础设施层（Repository impl）

pub mod ack_publisher;
pub mod dlq_publisher;
pub mod hook;
pub mod offline;
pub mod online;
pub mod retry;

pub use ack_publisher::{KafkaAckPublisher, NoopAckPublisher};
pub use dlq_publisher::KafkaDlqPublisher;
pub use offline::{NoopOfflinePushSender, OfflinePushSenderRef, build_offline_sender};
pub use online::{NoopOnlinePushSender, OnlinePushSenderRef, build_online_sender};
pub use retry::{RetryPolicy, RetryableError, execute_with_retry};
