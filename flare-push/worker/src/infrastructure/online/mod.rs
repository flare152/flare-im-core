pub mod noop;

use std::sync::Arc;

use async_trait::async_trait;
use flare_server_core::error::Result;

use crate::domain::models::PushDispatchTask;
use crate::domain::repositories::OnlinePushSender;

pub type OnlinePushSenderRef = Arc<dyn OnlinePushSender>;

pub fn build_online_sender() -> OnlinePushSenderRef {
    noop::NoopOnlinePushSender::shared()
}

pub use noop::NoopOnlinePushSender;
