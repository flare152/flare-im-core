use anyhow::{Result, anyhow};
use flare_im_core::utils::{TimelineMetadata, current_millis, extract_timeline_from_extra};
use flare_proto::storage::StoreMessageRequest;
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

        // 确保时间线信息嵌入到消息的 extra 中
        flare_im_core::utils::embed_timeline_in_extra(&mut message, &timeline);

        Ok(Self {
            session_id,
            message_id: message.id.clone(),
            message,
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
