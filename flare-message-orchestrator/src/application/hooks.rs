use std::collections::HashMap;
use std::time::SystemTime;

use flare_im_core::hooks::{HookContext, MessageDraft, MessageRecord};
use flare_proto::common::{RequestContext, TenantContext};
use flare_proto::communication_core::MessageType;
use flare_proto::storage::{Message, StoreMessageRequest};
use serde_json::json;

use crate::domain::message_submission::MessageSubmission;

fn tenant_id(tenant: &Option<TenantContext>, default: Option<&String>) -> String {
    tenant
        .as_ref()
        .and_then(|ctx| non_empty(ctx.tenant_id.clone()))
        .or_else(|| default.cloned())
        .unwrap_or_else(|| "default".to_string())
}

fn non_empty(value: String) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

fn extract_client_message_id(message: &Message) -> Option<String> {
    message
        .extra
        .get("client_message_id")
        .cloned()
        .filter(|id| !id.is_empty())
}

pub fn build_hook_context(
    request: &StoreMessageRequest,
    default_tenant: Option<&String>,
) -> HookContext {
    let mut ctx = HookContext::new(tenant_id(&request.tenant, default_tenant));

    ctx.session_id = non_empty(request.session_id.clone());
    ctx.tags = request.tags.clone();

    if let Some(RequestContext {
        request_id,
        trace_id,
        span_id,
        client_ip,
        user_agent,
        tags,
    }) = request.context.as_ref()
    {
        ctx.request_metadata
            .insert("request_id".into(), request_id.clone());
        ctx.request_metadata
            .insert("span_id".into(), span_id.clone());
        ctx.request_metadata
            .insert("client_ip".into(), client_ip.clone());
        ctx.request_metadata
            .insert("user_agent".into(), user_agent.clone());
        if !trace_id.is_empty() {
            ctx.trace_id = Some(trace_id.clone());
        }
        ctx.tags.extend(tags.clone());
    }

    if let Some(tenant) = request.tenant.as_ref() {
        ctx.attributes
            .entry("tenant_business_type".into())
            .or_insert(tenant.business_type.clone());
        ctx.attributes
            .entry("tenant_environment".into())
            .or_insert(tenant.environment.clone());
        ctx.attributes.extend(tenant.attributes.clone());
    }

    if let Some(message) = request.message.as_ref() {
        let message_type_label = detect_message_type(message);
        ctx.sender_id = non_empty(message.sender_id.clone());
        ctx.session_type = non_empty(message.session_type.clone());
        ctx.message_type = Some(message_type_label.to_string());

        ctx.attributes
            .entry("business_type".into())
            .or_insert(message.business_type.clone());
        ctx.attributes
            .entry("session_type".into())
            .or_insert(message.session_type.clone());
        ctx.attributes
            .entry("receiver_id".into())
            .or_insert(message.receiver_id.clone());
        if !message.receiver_ids.is_empty() {
            ctx.attributes
                .entry("receiver_ids".into())
                .or_insert(message.receiver_ids.join(","));
        }
        if let Some(client_msg_id) = extract_client_message_id(message) {
            ctx.attributes
                .entry("client_message_id".into())
                .or_insert(client_msg_id);
        }
        ctx.attributes
            .entry("message_type_label".into())
            .or_insert(message_type_label.to_string());
    }

    ctx.attributes
        .entry("sync".into())
        .or_insert(request.sync.to_string());

    ctx
}

pub fn build_draft_from_request(request: &StoreMessageRequest) -> MessageDraft {
    let message = request
        .message
        .as_ref()
        .expect("StoreMessageRequest.message must be set");

    let mut draft = MessageDraft::new(message.content.clone());
    let message_type_label = detect_message_type(message);

    if let Some(id) = non_empty(message.id.clone()) {
        draft.set_message_id(id);
    }

    if let Some(conv) = non_empty(request.session_id.clone()) {
        draft.set_conversation_id(conv);
    }

    draft.headers = request.tags.clone();

    let mut metadata = message.extra.clone();
    metadata
        .entry("business_type".into())
        .or_insert(message.business_type.clone());
    metadata
        .entry("session_type".into())
        .or_insert(message.session_type.clone());
    metadata
        .entry("message_type".into())
        .or_insert(message_type_label.to_string());
    metadata
        .entry("content_type".into())
        .or_insert(message.content_type.clone());
    metadata
        .entry("sender_id".into())
        .or_insert(message.sender_id.clone());
    metadata
        .entry("receiver_id".into())
        .or_insert(message.receiver_id.clone());
    metadata
        .entry("receiver_ids".into())
        .or_insert(if message.receiver_ids.is_empty() {
            String::new()
        } else {
            message.receiver_ids.join(",")
        });
    draft.metadata = metadata;

    draft.extra("session_id", json!(request.session_id));
    draft.extra("sync", json!(request.sync));
    draft.extra("receiver_ids", json!(message.receiver_ids));

    if let Some(context) = request.context.as_ref() {
        draft.extra(
            "request_context",
            json!({
                "request_id": context.request_id,
                "trace_id": context.trace_id,
                "span_id": context.span_id,
                "tags": context.tags,
            }),
        );
    }

    if let Some(tenant) = request.tenant.as_ref() {
        draft.extra(
            "tenant_context",
            json!({
                "tenant_id": tenant.tenant_id,
                "business_type": tenant.business_type,
                "environment": tenant.environment,
                "attributes": tenant.attributes,
            }),
        );
    }

    draft
}

pub fn apply_draft_to_request(request: &mut StoreMessageRequest, draft: &MessageDraft) {
    if let Some(conv) = draft.conversation_id.as_ref() {
        request.session_id = conv.clone();
    }

    request.tags = draft.headers.clone();

    if let Some(message) = request.message.as_mut() {
        if let Some(id) = draft.message_id.as_ref() {
            message.id = id.clone();
        }

        if let Some(conv) = draft.conversation_id.as_ref() {
            message.session_id = conv.clone();
        }

        message.content = draft.payload.clone();
        message.extra = draft.metadata.clone();
        if let Some(label) = message.extra.get("message_type") {
            message.content_type = label.clone();
        }

        for (key, value) in &draft.extra {
            if let Ok(serialized) = serde_json::to_string(value) {
                message.extra.insert(key.clone(), serialized);
            }
        }
    }
}

pub fn build_message_record(
    submission: &MessageSubmission,
    request: &StoreMessageRequest,
) -> MessageRecord {
    let envelope = &submission.stored_message.envelope;
    let mut metadata: HashMap<String, String> = submission.stored_message.payload.extra.clone();

    metadata.insert("business_type".into(), envelope.business_type.clone());
    metadata.insert("session_type".into(), envelope.session_type.clone());
    metadata.insert("content_type".into(), envelope.content_type.clone());
    metadata.insert("sender_type".into(), envelope.sender_type.clone());

    if let Some(message) = submission.kafka_payload.message.as_ref() {
        if let Some(client_msg_id) = extract_client_message_id(message) {
            metadata
                .entry("client_message_id".into())
                .or_insert(client_msg_id);
        }
    }

    for (key, value) in &request.tags {
        metadata.insert(format!("tag::{}", key), value.clone());
    }

    MessageRecord {
        message_id: envelope.message_id.clone(),
        client_message_id: None,
        conversation_id: envelope.session_id.clone(),
        sender_id: envelope.sender_id.clone(),
        session_type: Some(envelope.session_type.clone()),
        message_type: Some(envelope.content_type.clone()),
        persisted_at: SystemTime::now(),
        metadata,
    }
}

pub fn draft_from_submission(submission: &MessageSubmission) -> MessageDraft {
    build_draft_from_request(&submission.kafka_payload)
}

pub fn merge_context(original: &HookContext, mut updated: HookContext) -> HookContext {
    if updated.trace_id.is_none() {
        updated.trace_id = original.trace_id.clone();
    }
    if updated.sender_id.is_none() {
        updated.sender_id = original.sender_id.clone();
    }
    if updated.session_type.is_none() {
        updated.session_type = original.session_type.clone();
    }
    if updated.message_type.is_none() {
        updated.message_type = original.message_type.clone();
    }

    if updated.tags.is_empty() {
        updated.tags = original.tags.clone();
    }

    if updated.attributes.is_empty() {
        updated.attributes = original.attributes.clone();
    } else {
        for (key, value) in &original.attributes {
            updated
                .attributes
                .entry(key.clone())
                .or_insert(value.clone());
        }
    }

    if updated.request_metadata.is_empty() {
        updated.request_metadata = original.request_metadata.clone();
    }

    updated
}

fn detect_message_type(message: &Message) -> &'static str {
    match MessageType::from_i32(message.message_type) {
        Some(MessageType::Text) => "text",
        Some(MessageType::RichText) => "rich_text",
        Some(MessageType::Image) => "image",
        Some(MessageType::Video) => "video",
        Some(MessageType::Audio) => "audio",
        Some(MessageType::File) => "file",
        Some(MessageType::Sticker) => "sticker",
        Some(MessageType::Location) => "location",
        Some(MessageType::Card) => "card",
        Some(MessageType::Command) => "command",
        Some(MessageType::Event) => "event",
        Some(MessageType::System) => "system",
        Some(MessageType::Custom) | Some(MessageType::Unspecified) | None => {
            infer_from_content_type(&message.content_type)
        }
    }
}

fn infer_from_content_type(raw: &str) -> &'static str {
    match raw.trim().to_lowercase().as_str() {
        "text/plain" | "text" | "plain_text" => "text",
        "markdown" | "text/markdown" | "rich_text" | "rich-text" => "rich_text",
        "image" | "image/png" | "image/jpeg" | "image/jpg" => "image",
        "video" | "video/mp4" | "video/mpeg" => "video",
        "audio" | "audio/aac" | "audio/mpeg" | "voice" => "audio",
        "file" | "application/octet-stream" | "application/pdf" | "application/zip" => "file",
        "sticker" | "emoji" | "gif" => "sticker",
        "location" | "geo" | "geolocation" => "location",
        "card" | "share_card" | "invite_card" => "card",
        "command" | "cmd" => "command",
        "event" => "event",
        "system" | "system_message" => "system",
        _ => "custom",
    }
}
