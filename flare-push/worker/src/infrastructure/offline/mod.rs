pub mod noop;

use std::sync::Arc;

use crate::domain::repository::OfflinePushSender;

pub type OfflinePushSenderRef = Arc<dyn OfflinePushSender>;

pub fn build_offline_sender() -> OfflinePushSenderRef {
    noop::NoopOfflinePushSender::shared()
}

pub use noop::NoopOfflinePushSender;
