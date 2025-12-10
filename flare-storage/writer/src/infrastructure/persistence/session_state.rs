use std::sync::Arc;
use async_trait::async_trait;

use anyhow::Result;
use redis::{AsyncCommands, aio::ConnectionManager};

use crate::domain::repository::SessionStateRepository;

pub struct RedisSessionStateRepository {
    client: Arc<redis::Client>,
}

impl RedisSessionStateRepository {
    pub fn new(client: Arc<redis::Client>) -> Self {
        Self { client }
    }

    async fn connection(&self) -> Result<ConnectionManager> {
        Ok(ConnectionManager::new(self.client.as_ref().clone()).await?)
    }
}


#[async_trait]
impl SessionStateRepository for RedisSessionStateRepository {
    async fn apply_message(&self, message: &flare_proto::common::Message) -> Result<()> {
        let mut conn = self.connection().await?;

        let session_id = &message.session_id;
        let state_key = format!("storage:session:state:{}", session_id);
        let unread_key = format!("storage:session:unread:{}", session_id);

        // 从 extra 中提取时间线信息
        let timeline = flare_im_core::utils::extract_timeline_from_extra(
            &message.extra,
            flare_im_core::utils::current_millis(),
        );

        // 推断 content_type
        let content_type = message.content.as_ref()
            .map(|c| match &c.content {
                Some(flare_proto::common::message_content::Content::Text(_)) => "text/plain",
                Some(flare_proto::common::message_content::Content::Image(_)) => "image/*",
                Some(flare_proto::common::message_content::Content::Video(_)) => "video/*",
                Some(flare_proto::common::message_content::Content::Audio(_)) => "audio/*",
                Some(flare_proto::common::message_content::Content::File(_)) => "application/octet-stream",
                Some(flare_proto::common::message_content::Content::Location(_)) => "application/location",
                Some(flare_proto::common::message_content::Content::Card(_)) => "application/card",
                Some(flare_proto::common::message_content::Content::Notification(_)) => "application/notification",
                Some(flare_proto::common::message_content::Content::Custom(_)) => "application/custom",
                Some(flare_proto::common::message_content::Content::Forward(_)) => "application/forward",
                Some(flare_proto::common::message_content::Content::Typing(_)) => "application/typing",
                Some(flare_proto::common::message_content::Content::SystemEvent(_)) => "application/system_event",
                Some(flare_proto::common::message_content::Content::Quote(_)) => "application/quote",
                Some(flare_proto::common::message_content::Content::LinkCard(_)) => "application/link_card",
                None => "application/unknown",
            })
            .unwrap_or("application/unknown");

        let last_message_id = message.id.clone();
        let last_sender_id = message.sender_id.clone();
        let last_type = message.message_type.to_string();
        let last_content_type = content_type.to_string();
        let last_ts = timeline.ingestion_ts.to_string();

        let _: () = conn
            .hset_multiple(
                &state_key,
                &[
                    ("last_message_id", last_message_id.as_str()),
                    ("last_sender_id", last_sender_id.as_str()),
                    ("last_message_type", last_type.as_str()),
                    ("last_content_type", last_content_type.as_str()),
                    ("last_message_ts", last_ts.as_str()),
                ],
            )
            .await?;

        // unread logic: receiver_id/receiver_ids 已废弃，通过 session_id 确定接收者
        // 此处需要通过 session 服务查询参与者，暂时保留逻辑但不执行
        // TODO: 通过 session_id 查询参与者列表，然后更新 unread
        // sender unread reset
        let _: () = conn
            .hset(&unread_key, &message.sender_id, 0i64)
            .await?;

        Ok(())
    }
}
