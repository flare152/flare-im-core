pub mod noop;

use std::sync::Arc;

use crate::domain::repository::OnlinePushSender;

pub type OnlinePushSenderRef = Arc<dyn OnlinePushSender>;

pub fn build_online_sender() -> OnlinePushSenderRef {
    noop::NoopOnlinePushSender::shared()
}

pub use noop::NoopOnlinePushSender;
