use anyhow::Result;
use async_trait::async_trait;
use flare_proto::push::{PushMessageRequest, PushNotificationRequest};

/// 推送事件发布器（需要作为 trait 对象使用，保留 async-trait）
#[async_trait]
pub trait PushEventPublisher: Send + Sync {
    async fn publish_message(&self, request: &PushMessageRequest) -> Result<()>;
    async fn publish_notification(&self, request: &PushNotificationRequest) -> Result<()>;
}
