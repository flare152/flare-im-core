use anyhow::{Result, anyhow};
use flare_proto::storage::StoreMessageRequest;
use flare_storage_model::{
    StoredMessage, TimelineMetadata, current_millis, extract_timeline_from_extra,
};
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize)]
pub struct MediaAttachmentMetadata {
    pub file_id: String,
    pub file_name: String,
    pub mime_type: String,
    pub size: i64,
    pub url: String,
    pub cdn_url: String,
}

pub struct PreparedMessage {
    pub session_id: String,
    pub message_id: String,
    pub message: flare_proto::storage::Message,
    pub stored_message: StoredMessage,
    pub timeline: TimelineMetadata,
    pub sync: bool,
}

impl PreparedMessage {
    pub fn from_request(request: StoreMessageRequest) -> Result<Self> {
        let session_id = if request.session_id.is_empty() {
            request
                .message
                .as_ref()
                .map(|msg| msg.session_id.clone())
                .unwrap_or_default()
        } else {
            request.session_id.clone()
        };

        let mut message = request
            .message
            .ok_or_else(|| anyhow!("missing message payload"))?;

        if message.session_id.is_empty() {
            message.session_id = session_id.clone();
        }

        if message.id.is_empty() {
            message.id = Uuid::new_v4().to_string();
        }

        let mut timeline = extract_timeline_from_extra(&message.extra, current_millis());
        let persisted_ts = current_millis();
        timeline.persisted_ts = Some(persisted_ts);

        let shard_key = message
            .extra
            .get("shard_key")
            .cloned()
            .unwrap_or_else(|| session_id.clone());
        let tenant_id = message.extra.get("tenant_id").cloned();

        let stored_message =
            StoredMessage::from_proto(&message, timeline.clone(), shard_key, tenant_id);

        Ok(Self {
            session_id,
            message_id: message.id.clone(),
            message,
            stored_message,
            timeline,
            sync: request.sync,
        })
    }
}

pub struct PersistenceResult {
    pub session_id: String,
    pub message_id: String,
    pub timeline: TimelineMetadata,
    pub deduplicated: bool,
}

impl PersistenceResult {
    pub fn new(prepared: &PreparedMessage, deduplicated: bool) -> Self {
        Self {
            session_id: prepared.session_id.clone(),
            message_id: prepared.message_id.clone(),
            timeline: prepared.timeline.clone(),
            deduplicated,
        }
    }
}
