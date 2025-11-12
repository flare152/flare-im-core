use anyhow::Result;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use chrono::{DateTime, TimeZone, Utc};
use flare_proto::communication_core::MessageContent as ProtoMessageContent;
use flare_proto::storage::{Message, MessageOperation, MessageReadRecord};
use prost::Message as _;
use prost_types::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredMessage {
    pub envelope: MessageEnvelope,
    pub timeline: TimelineMetadata,
    pub payload: MessagePayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEnvelope {
    pub message_id: String,
    pub session_id: String,
    pub shard_key: String,
    pub tenant_id: Option<String>,
    pub business_type: String,
    pub session_type: String,
    pub sender_id: String,
    pub sender_type: String,
    pub receiver_scope: ReceiverScope,
    pub content_type: String,
    pub message_type: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiverScope {
    pub primary: Option<String>,
    pub targets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TimelineMetadata {
    pub emit_ts: Option<i64>,
    pub ingestion_ts: i64,
    pub persisted_ts: Option<i64>,
    pub dispatched_ts: Option<i64>,
    pub acked_ts: Option<i64>,
    pub read_ts: Option<i64>,
    pub deleted_ts: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagePayload {
    pub status: String,
    pub data_base64: String,
    pub structured_content_base64: Option<String>,
    pub extra: HashMap<String, String>,
    pub is_recalled: bool,
    pub recalled_at: Option<i64>,
    pub recall_reason: Option<String>,
    pub is_burn_after_read: bool,
    pub burn_after_seconds: i32,
    pub read_by: Vec<StoredReadRecord>,
    pub visibility: HashMap<String, i32>,
    pub operations: Vec<StoredOperation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredReadRecord {
    pub user_id: String,
    pub read_at: Option<i64>,
    pub burned_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredOperation {
    pub operation_type: String,
    pub operator_id: String,
    pub operated_at: Option<i64>,
    pub target_user_id: String,
    pub metadata: HashMap<String, String>,
}

impl StoredMessage {
    pub fn from_proto(
        message: &Message,
        timeline: TimelineMetadata,
        shard_key: String,
        tenant_id: Option<String>,
    ) -> Self {
        let envelope = MessageEnvelope {
            message_id: message.id.clone(),
            session_id: message.session_id.clone(),
            shard_key,
            tenant_id,
            business_type: message.business_type.clone(),
            session_type: message.session_type.clone(),
            sender_id: message.sender_id.clone(),
            sender_type: message.sender_type.clone(),
            receiver_scope: ReceiverScope {
                primary: if message.receiver_id.is_empty() {
                    None
                } else {
                    Some(message.receiver_id.clone())
                },
                targets: message.receiver_ids.clone(),
            },
            content_type: message.content_type.clone(),
            message_type: message.message_type,
        };

        let payload = MessagePayload {
            status: message.status.clone(),
            data_base64: BASE64.encode(&message.content),
            structured_content_base64: message
                .structured_content
                .as_ref()
                .and_then(|structured| encode_structured_content(structured)),
            extra: message.extra.clone(),
            is_recalled: message.is_recalled,
            recalled_at: message.recalled_at.as_ref().and_then(timestamp_to_millis),
            recall_reason: if message.recall_reason.is_empty() {
                None
            } else {
                Some(message.recall_reason.clone())
            },
            is_burn_after_read: message.is_burn_after_read,
            burn_after_seconds: message.burn_after_seconds,
            read_by: message.read_by.iter().map(StoredReadRecord::from).collect(),
            visibility: message.visibility.clone(),
            operations: message
                .operations
                .iter()
                .map(StoredOperation::from)
                .collect(),
        };

        Self {
            envelope,
            timeline,
            payload,
        }
    }

    pub fn into_proto(self) -> Result<Message> {
        let Self {
            envelope,
            timeline,
            payload,
        } = self;

        let tenant_context = envelope.tenant_id.map(|tenant_id| {
            let mut ctx = flare_proto::common::TenantContext::default();
            ctx.tenant_id = tenant_id;
            ctx
        });

        let mut message = Message {
            id: envelope.message_id,
            session_id: envelope.session_id,
            sender_id: envelope.sender_id,
            sender_type: envelope.sender_type,
            receiver_ids: envelope.receiver_scope.targets,
            receiver_id: envelope.receiver_scope.primary.unwrap_or_default(),
            content: BASE64.decode(payload.data_base64)?,
            content_type: envelope.content_type,
            message_type: envelope.message_type,
            timestamp: timeline
                .emit_ts
                .or(Some(timeline.ingestion_ts))
                .and_then(millis_to_timestamp),
            status: payload.status,
            extra: payload.extra,
            business_type: envelope.business_type,
            session_type: envelope.session_type,
            is_recalled: payload.is_recalled,
            recalled_at: payload.recalled_at.and_then(millis_to_timestamp),
            recall_reason: payload.recall_reason.unwrap_or_default(),
            is_burn_after_read: payload.is_burn_after_read,
            burn_after_seconds: payload.burn_after_seconds,
            read_by: payload
                .read_by
                .into_iter()
                .map(MessageReadRecord::from)
                .collect(),
            visibility: payload.visibility,
            operations: payload
                .operations
                .into_iter()
                .map(MessageOperation::from)
                .collect(),
            tenant: tenant_context,
            attachments: Vec::new(),
            audit: None,
            structured_content: payload
                .structured_content_base64
                .and_then(decode_structured_content),
        };

        // timeline metadata is stored in extra for roundtrip
        embed_timeline_in_extra(&mut message, &timeline);

        Ok(message)
    }
}

impl StoredReadRecord {
    fn from(record: &MessageReadRecord) -> Self {
        StoredReadRecord {
            user_id: record.user_id.clone(),
            read_at: record.read_at.as_ref().and_then(timestamp_to_millis),
            burned_at: record.burned_at.as_ref().and_then(timestamp_to_millis),
        }
    }
}

impl From<StoredReadRecord> for MessageReadRecord {
    fn from(value: StoredReadRecord) -> Self {
        MessageReadRecord {
            user_id: value.user_id,
            read_at: value.read_at.and_then(millis_to_timestamp),
            burned_at: value.burned_at.and_then(millis_to_timestamp),
        }
    }
}

impl StoredOperation {
    fn from(operation: &MessageOperation) -> Self {
        StoredOperation {
            operation_type: operation.operation_type.clone(),
            operator_id: operation.operator_id.clone(),
            operated_at: operation.operated_at.as_ref().and_then(timestamp_to_millis),
            target_user_id: operation.target_user_id.clone(),
            metadata: operation.metadata.clone(),
        }
    }
}

impl From<StoredOperation> for MessageOperation {
    fn from(value: StoredOperation) -> Self {
        MessageOperation {
            operation_type: value.operation_type,
            operator_id: value.operator_id,
            operated_at: value.operated_at.and_then(millis_to_timestamp),
            target_user_id: value.target_user_id,
            metadata: value.metadata,
        }
    }
}

pub fn timestamp_to_millis(ts: &Timestamp) -> Option<i64> {
    if ts.seconds == 0 && ts.nanos == 0 {
        return None;
    }
    Some(ts.seconds * 1000 + (ts.nanos as i64 / 1_000_000))
}

pub fn millis_to_timestamp(ms: i64) -> Option<Timestamp> {
    let seconds = ms / 1000;
    let nanos = ((ms % 1000) * 1_000_000) as i32;
    Some(Timestamp { seconds, nanos })
}

fn embed_timeline_in_extra(message: &mut Message, timeline: &TimelineMetadata) {
    let mut timeline_map = HashMap::new();
    if let Some(value) = timeline.emit_ts {
        timeline_map.insert("emit_ts".to_string(), value.to_string());
    }
    timeline_map.insert(
        "ingestion_ts".to_string(),
        timeline.ingestion_ts.to_string(),
    );
    if let Some(value) = timeline.persisted_ts {
        timeline_map.insert("persisted_ts".to_string(), value.to_string());
    }
    if let Some(value) = timeline.dispatched_ts {
        timeline_map.insert("dispatched_ts".to_string(), value.to_string());
    }
    if let Some(value) = timeline.acked_ts {
        timeline_map.insert("acked_ts".to_string(), value.to_string());
    }
    if let Some(value) = timeline.read_ts {
        timeline_map.insert("read_ts".to_string(), value.to_string());
    }
    if let Some(value) = timeline.deleted_ts {
        timeline_map.insert("deleted_ts".to_string(), value.to_string());
    }

    let json = serde_json::to_string(&timeline_map).unwrap_or_default();
    message.extra.insert("timeline".to_string(), json);
}

pub fn extract_timeline_from_extra(
    extra: &HashMap<String, String>,
    default_ingestion_ts: i64,
) -> TimelineMetadata {
    if let Some(raw) = extra.get("timeline") {
        if let Ok(map) = serde_json::from_str::<HashMap<String, String>>(raw) {
            return TimelineMetadata {
                emit_ts: map.get("emit_ts").and_then(parse_i64),
                ingestion_ts: map
                    .get("ingestion_ts")
                    .and_then(parse_i64)
                    .unwrap_or(default_ingestion_ts),
                persisted_ts: map.get("persisted_ts").and_then(parse_i64),
                dispatched_ts: map.get("dispatched_ts").and_then(parse_i64),
                acked_ts: map.get("acked_ts").and_then(parse_i64),
                read_ts: map.get("read_ts").and_then(parse_i64),
                deleted_ts: map.get("deleted_ts").and_then(parse_i64),
            };
        }
    }

    TimelineMetadata {
        ingestion_ts: default_ingestion_ts,
        ..TimelineMetadata::default()
    }
}

fn parse_i64(value: &String) -> Option<i64> {
    value.parse::<i64>().ok()
}

fn encode_structured_content(content: &ProtoMessageContent) -> Option<String> {
    let mut buffer = Vec::new();
    if content.encode(&mut buffer).is_ok() {
        Some(BASE64.encode(buffer))
    } else {
        None
    }
}

fn decode_structured_content(encoded: String) -> Option<ProtoMessageContent> {
    let bytes = BASE64.decode(encoded).ok()?;
    ProtoMessageContent::decode(bytes.as_slice()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use flare_proto::communication_core::{MessageContent, TextContent};
    use flare_proto::storage::Message;
    use prost::Message as _;

    #[test]
    fn roundtrip_structured_content() {
        let mut message = Message::default();
        message.id = "msg-1".into();
        message.session_id = "session-1".into();
        message.sender_id = "user-1".into();
        message.sender_type = "user".into();
        message.message_type = 1;
        message.structured_content = Some(MessageContent {
            content: Some(
                flare_proto::communication_core::message_content::Content::Text(TextContent {
                    text: "hello".into(),
                    mentions: vec![],
                }),
            ),
        });

        let timeline = TimelineMetadata {
            ingestion_ts: current_millis(),
            ..TimelineMetadata::default()
        };

        let stored =
            StoredMessage::from_proto(&message, timeline.clone(), "session-1".into(), None);
        assert!(stored.payload.structured_content_base64.is_some());

        let restored = stored.into_proto().unwrap();
        let encoded = restored
            .structured_content
            .as_ref()
            .map(|content| content.encode_to_vec())
            .unwrap();
        assert!(!encoded.is_empty());
        assert_eq!(restored.message_type, 1);
    }
}

pub fn current_millis() -> i64 {
    Utc::now().timestamp_millis()
}

pub fn timestamp_to_datetime(ts: &Timestamp) -> Option<DateTime<Utc>> {
    timestamp_to_millis(ts).and_then(|ms| Some(Utc.timestamp_millis_opt(ms).single()?))
}

pub fn datetime_to_timestamp(dt: DateTime<Utc>) -> Timestamp {
    Timestamp {
        seconds: dt.timestamp(),
        nanos: dt.timestamp_subsec_nanos() as i32,
    }
}
