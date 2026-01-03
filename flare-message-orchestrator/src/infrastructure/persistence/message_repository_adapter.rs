use anyhow::{Context, Result};
use std::sync::Arc;
use flare_proto::storage::storage_reader_service_client::StorageReaderServiceClient;
use flare_proto::storage::{GetMessageRequest, GetMessageResponse};
use tonic::Request;
use crate::domain::model::{Message, MessageFsmState};
use crate::domain::service::message_operation_service::MessageRepository;
use chrono::{DateTime, Utc};
use prost_types::Timestamp;

pub struct StorageReaderMessageRepository {
    client: StorageReaderServiceClient<tonic::transport::Channel>,
}

impl StorageReaderMessageRepository {
    pub fn new(client: Arc<StorageReaderServiceClient<tonic::transport::Channel>>) -> Self {
        Self { client: (*client).clone() }
    }
}

#[async_trait::async_trait]
impl MessageRepository for StorageReaderMessageRepository {
    async fn find_by_id(&self, message_id: &str) -> Result<Option<Message>> {
        let req = GetMessageRequest {
            message_id: message_id.to_string(),
            context: None,
            tenant: None,
        };

        let mut client = self.client.clone();
        let resp = client.get_message(Request::new(req)).await?;
        let inner: GetMessageResponse = resp.into_inner();

        if let Some(proto_msg) = inner.message {
            let fsm_state = if proto_msg.is_recalled {
                MessageFsmState::Recalled
            } else if proto_msg.status == flare_proto::common::MessageStatus::DeletedHard as i32 {
                MessageFsmState::DeletedHard
            } else {
                MessageFsmState::from_str(
                    proto_msg.extra.get("message_fsm_state")
                        .map(|s| s.as_str())
                        .unwrap_or("SENT")
                ).unwrap_or(MessageFsmState::Sent)
            };

            let timestamp = proto_msg.timestamp
                .map(|ts| {
                    DateTime::from_timestamp(ts.seconds, ts.nanos as u32)
                        .unwrap_or_else(Utc::now)
                })
                .unwrap_or_else(Utc::now);

            // 将 MessageContent 序列化为二进制
            let content_bytes = proto_msg.content
                .as_ref()
                .map(|c| {
                    use prost::Message;
                    let mut buf = Vec::new();
                    c.encode(&mut buf).unwrap_or_default();
                    buf
                })
                .unwrap_or_default();

            let message = Message {
                server_id: proto_msg.server_id.clone(),
                conversation_id: proto_msg.conversation_id.clone(),
                sender_id: proto_msg.sender_id.clone(),
                content: content_bytes,
                timestamp,
                fsm_state,
                fsm_state_changed_at: timestamp,
                edit_version: proto_msg.extra.get("current_edit_version")
                    .and_then(|v| v.parse::<i32>().ok())
                    .unwrap_or(0),
                edit_history: vec![],
                updated_at: timestamp,
            };

            Ok(Some(message))
        } else {
            Ok(None)
        }
    }

    async fn save(&self, _message: &Message) -> Result<()> {
        Err(anyhow::anyhow!("Save operation should be handled by Writer via Kafka"))
    }
}

