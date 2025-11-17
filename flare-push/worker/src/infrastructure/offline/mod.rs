pub mod noop;

use std::sync::Arc;

use async_trait::async_trait;
use flare_server_core::error::Result;

use crate::domain::models::PushDispatchTask;
use crate::domain::repositories::OfflinePushSender;

pub type OfflinePushSenderRef = Arc<dyn OfflinePushSender>;

pub fn build_offline_sender() -> OfflinePushSenderRef {
    noop::NoopOfflinePushSender::shared()
}

pub use noop::NoopOfflinePushSender;
