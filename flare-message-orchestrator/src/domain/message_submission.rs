use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use flare_proto::storage::StoreMessageRequest;
use flare_storage_model::{
    StoredMessage, TimelineMetadata, current_millis, datetime_to_timestamp, timestamp_to_millis,
};
use uuid::Uuid;

use crate::domain::message_kind::MessageProfile;

#[derive(Clone, Debug)]
pub struct MessageDefaults {
    pub default_business_type: String,
    pub default_session_type: String,
    pub default_sender_type: String,
    pub default_tenant_id: Option<String>,
}

#[derive(Clone)]
pub struct MessageSubmission {
    pub kafka_payload: StoreMessageRequest,
    pub stored_message: StoredMessage,
    pub message_id: String,
}

impl MessageSubmission {
    pub fn prepare(mut request: StoreMessageRequest, defaults: &MessageDefaults) -> Result<Self> {
        if request.session_id.is_empty() {
            return Err(anyhow!("session_id is required"));
        }

        let mut message = request
            .message
            .take()
            .ok_or_else(|| anyhow!("message payload is required"))?;

        if message.session_id.is_empty() {
            message.session_id = request.session_id.clone();
        }

        if message.id.is_empty() {
            message.id = Uuid::new_v4().to_string();
        }

        if message.sender_id.is_empty() {
            return Err(anyhow!("sender_id is required"));
        }

        if message.sender_type.is_empty() {
            message.sender_type = defaults.default_sender_type.clone();
        }

        if message.business_type.is_empty() {
            message.business_type = defaults.default_business_type.clone();
        }

        if message.session_type.is_empty() {
            message.session_type = defaults.default_session_type.clone();
        }

        if message.status.is_empty() {
            message.status = "created".to_string();
        }

        let profile = MessageProfile::ensure(&mut message);
        if message.extra.get("message_type").is_none() {
            message.extra.insert(
                "message_type".into(),
                profile.message_type_label().to_string(),
            );
        }

        if message.timestamp.is_none() {
            message.timestamp = Some(datetime_to_timestamp(Utc::now()));
        }

        let ingestion_ts = current_millis();
        let emit_ts = message
            .timestamp
            .as_ref()
            .and_then(|ts| timestamp_to_millis(ts));

        let shard_key = message
            .extra
            .get("shard_key")
            .cloned()
            .unwrap_or_else(|| request.session_id.clone());
        message
            .extra
            .entry("shard_key".to_string())
            .or_insert(shard_key.clone());

        let tenant_id = message
            .extra
            .get("tenant_id")
            .cloned()
            .or_else(|| defaults.default_tenant_id.clone());
        if let Some(ref tenant) = tenant_id {
            message
                .extra
                .entry("tenant_id".to_string())
                .or_insert(tenant.clone());
        }

        message
            .extra
            .entry("ingestion_ts".to_string())
            .or_insert(ingestion_ts.to_string());

        let stored_message = StoredMessage::from_proto(
            &message,
            TimelineMetadata {
                emit_ts,
                ingestion_ts,
                ..TimelineMetadata::default()
            },
            shard_key,
            tenant_id,
        );

        let kafka_message = stored_message
            .clone()
            .into_proto()
            .context("failed to convert stored message back to proto")?;

        let message_id = kafka_message.id.clone();

        let kafka_payload = StoreMessageRequest {
            session_id: request.session_id,
            message: Some(kafka_message),
            sync: request.sync,
            context: request.context,
            tenant: request.tenant,
            tags: request.tags,
        };

        Ok(Self {
            kafka_payload,
            stored_message,
            message_id,
        })
    }
}
