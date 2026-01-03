use async_trait::async_trait;
use std::sync::Arc;

use anyhow::Result;
use redis::{AsyncCommands, aio::ConnectionManager};
use tracing::warn;

use crate::domain::repository::ConversationStateRepository;
use crate::domain::service::MessagePersistenceDomainService;

pub struct RedisConversationStateRepository {
    client: Arc<redis::Client>,
    domain_service: Option<Arc<MessagePersistenceDomainService>>,
}

impl RedisConversationStateRepository {
    pub fn new(client: Arc<redis::Client>) -> Self {
        Self {
            client,
            domain_service: None,
        }
    }

    pub fn with_domain_service(
        mut self,
        domain_service: Option<Arc<MessagePersistenceDomainService>>,
    ) -> Self {
        self.domain_service = domain_service;
        self
    }

    async fn connection(&self) -> Result<ConnectionManager> {
        Ok(ConnectionManager::new(self.client.as_ref().clone()).await?)
    }
}

#[async_trait]
impl ConversationStateRepository for RedisConversationStateRepository {
    async fn apply_message(&self, message: &flare_proto::common::Message) -> Result<()> {
        let mut conn = self.connection().await?;

        let conversation_id = &message.conversation_id;
        let state_key = format!("storage:conversation:state:{}", conversation_id);
        let unread_key = format!("storage:conversation:unread:{}", conversation_id);

        // 从 extra 中提取时间线信息
        let timeline = flare_im_core::utils::extract_timeline_from_extra(
            &message.extra,
            flare_im_core::utils::current_millis(),
        );

        // 推断 content_type
        let content_type = message
            .content
            .as_ref()
            .map(|c| match &c.content {
                Some(flare_proto::common::message_content::Content::Text(_)) => "text/plain",
                Some(flare_proto::common::message_content::Content::Image(_)) => "image/*",
                Some(flare_proto::common::message_content::Content::Video(_)) => "video/*",
                Some(flare_proto::common::message_content::Content::Audio(_)) => "audio/*",
                Some(flare_proto::common::message_content::Content::File(_)) => {
                    "application/octet-stream"
                }
                Some(flare_proto::common::message_content::Content::Location(_)) => {
                    "application/location"
                }
                Some(flare_proto::common::message_content::Content::Card(_)) => "application/card",
                // Quote 不在 Content oneof 中，而是作为 Message.quote 字段
                Some(flare_proto::common::message_content::Content::LinkCard(_)) => {
                    "application/link_card"
                }
                Some(flare_proto::common::message_content::Content::Forward(_)) => {
                    "application/forward"
                }
                Some(flare_proto::common::message_content::Content::Thread(_)) => {
                    "application/thread"
                }
                Some(flare_proto::common::message_content::Content::Custom(_)) => {
                    "application/custom"
                }
                Some(flare_proto::common::message_content::Content::Notification(_)) => {
                    "application/notification"
                }
                Some(flare_proto::common::message_content::Content::Typing(_)) => {
                    "application/typing"
                }
                Some(flare_proto::common::message_content::Content::SystemEvent(_)) => {
                    "application/system_event"
                }
                Some(flare_proto::common::message_content::Content::Operation(_)) => {
                    "application/operation"
                }
                None => "application/unknown",
            })
            .unwrap_or("application/unknown");

        let last_message_id = message.server_id.clone();
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

        // 重置发送者的未读数
        let _: () = conn.hset(&unread_key, &message.sender_id, 0i64).await?;

        // 通过 conversation_id 查询参与者列表，然后更新其他参与者的未读数
        if let Some(domain_service) = &self.domain_service {
            match domain_service.get_conversation_participants(conversation_id).await {
                Ok(participant_ids) => {
                    // 更新除发送者外的所有参与者的未读数
                    for participant_id in participant_ids {
                        if participant_id != message.sender_id {
                            // 增加该参与者的未读数
                            let current_unread: i64 =
                                conn.hget(&unread_key, &participant_id).await.unwrap_or(0);
                            let _: () = conn
                                .hset(&unread_key, &participant_id, current_unread + 1)
                                .await?;
                        }
                    }
                }
                Err(e) => {
                    warn!(error = ?e, conversation_id = %conversation_id, "Failed to get session participants");
                }
            }
        } else {
            warn!(conversation_id = %conversation_id, "Domain service not configured for session state repository");
        }

        Ok(())
    }
}
