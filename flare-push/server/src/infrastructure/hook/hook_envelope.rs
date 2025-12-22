use std::collections::HashMap;
use std::time::SystemTime;

use flare_im_core::hooks::{HookContext, MessageDraft, MessageRecord};
use flare_proto::common::{RequestContext, TenantContext};
use flare_proto::push::PushOptions;
use flare_server_core::error::{ErrorBuilder, ErrorCode, Result};
use uuid::Uuid;

use crate::domain::model::DispatchNotification;

const SESSION_TYPE_PUSH: &str = "push";
const SENDER_PUSH_SERVER: &str = "push-server";
const CONTENT_TYPE_JSON: &str = "application/json";
const CONTENT_TYPE_BINARY: &str = "application/octet-stream";

/// Hook 编排所需的 envelope，保存 HookContext 及核心元数据。
pub struct PushHookEnvelope {
    context: HookContext,
    message_id: String,
    conversation_id: String,
    message_type: String,
    tenant_id: String,
}

impl PushHookEnvelope {
    pub fn hook_context(&self) -> &HookContext {
        &self.context
    }

    pub fn message_id(&self, draft: &MessageDraft) -> String {
        draft
            .message_id
            .clone()
            .unwrap_or_else(|| self.message_id.clone())
    }

    pub fn message_type(&self) -> &str {
        &self.message_type
    }

    fn conversation_id(&self) -> &str {
        &self.conversation_id
    }
}

/// 构造消息类推送的 Hook envelope。
pub fn prepare_message_envelope(
    default_tenant_id: &str,
    tenant: Option<&TenantContext>,
    user_id: &str,
    request: Option<&RequestContext>,
    options: Option<&PushOptions>,
    payload: Vec<u8>,
) -> (PushHookEnvelope, MessageDraft) {
    prepare_envelope(
        default_tenant_id,
        tenant,
        user_id,
        "push_message",
        request,
        options,
        payload,
        CONTENT_TYPE_BINARY,
        HashMap::new(),
    )
}

/// 构造通知类推送的 Hook envelope。
pub fn prepare_notification_envelope(
    default_tenant_id: &str,
    tenant: Option<&TenantContext>,
    user_id: &str,
    request: Option<&RequestContext>,
    options: Option<&PushOptions>,
    notification: &DispatchNotification,
) -> Result<(PushHookEnvelope, MessageDraft)> {
    let mut extra = HashMap::new();
    extra.insert("notification.title".to_string(), notification.title.clone());
    extra.insert("notification.body".to_string(), notification.body.clone());
    for (key, value) in &notification.data {
        extra.insert(format!("notification.data.{key}"), value.clone());
    }
    for (key, value) in &notification.metadata {
        extra.insert(format!("notification.meta.{key}"), value.clone());
    }

    let payload = serde_json::to_vec(notification).map_err(|err| {
        ErrorBuilder::new(
            ErrorCode::SerializationError,
            "failed to encode notification draft",
        )
        .details(err.to_string())
        .build_error()
    })?;

    Ok(prepare_envelope(
        default_tenant_id,
        tenant,
        user_id,
        "push_notification",
        request,
        options,
        payload,
        CONTENT_TYPE_JSON,
        extra,
    ))
}

/// 将 Hook 执行后的草稿恢复为推送通知。
pub fn finalize_notification(draft: &MessageDraft) -> Result<DispatchNotification> {
    serde_json::from_slice(&draft.payload).map_err(|err| {
        ErrorBuilder::new(
            ErrorCode::DeserializationError,
            "failed to decode notification draft",
        )
        .details(err.to_string())
        .build_error()
    })
}

/// 基于 Hook envelope 构造 PostSend Hook 所需的 MessageRecord。
pub fn build_post_send_record(envelope: &PushHookEnvelope, draft: &MessageDraft) -> MessageRecord {
    MessageRecord {
        message_id: envelope.message_id(draft),
        client_message_id: draft.client_message_id.clone(),
        conversation_id: envelope.conversation_id().to_string(),
        sender_id: SENDER_PUSH_SERVER.to_string(),
        conversation_type: Some(SESSION_TYPE_PUSH.to_string()),
        message_type: Some(envelope.message_type().to_string()),
        persisted_at: SystemTime::now(),
        metadata: draft.metadata.clone(),
    }
}

fn prepare_envelope(
    default_tenant_id: &str,
    tenant: Option<&TenantContext>,
    user_id: &str,
    message_type: &str,
    request: Option<&RequestContext>,
    options: Option<&PushOptions>,
    payload: Vec<u8>,
    _default_content_type: &str,
    mut extra_metadata: HashMap<String, String>,
) -> (PushHookEnvelope, MessageDraft) {
    let tenant_id = tenant
        .and_then(|t| (!t.tenant_id.is_empty()).then_some(t.tenant_id.clone()))
        .unwrap_or_else(|| default_tenant_id.to_string());
    let conversation_id = format!("{SESSION_TYPE_PUSH}:{user_id}");

    if let Some(req) = request {
        if let Some(trace_ctx) = req.trace.as_ref() {
            if !trace_ctx.trace_id.is_empty() {
                extra_metadata.insert("trace_id".to_string(), trace_ctx.trace_id.clone());
            }
            if !trace_ctx.span_id.is_empty() {
                extra_metadata.insert("span_id".to_string(), trace_ctx.span_id.clone());
            }
        }
        if let Some(device_ctx) = req.device.as_ref() {
            if !device_ctx.ip_address.is_empty() {
                extra_metadata.insert("client_ip".to_string(), device_ctx.ip_address.clone());
            }
        }
        if !req.user_agent.is_empty() {
            extra_metadata.insert("user_agent".to_string(), req.user_agent.clone());
        }
    }

    if let Some(opts) = options {
        extra_metadata.insert(
            "push.require_online".to_string(),
            opts.require_online.to_string(),
        );
        extra_metadata.insert(
            "push.persist_if_offline".to_string(),
            opts.persist_if_offline.to_string(),
        );
        extra_metadata.insert("push.priority".to_string(), opts.priority.to_string());
        for (key, value) in &opts.metadata {
            extra_metadata.insert(format!("option.{key}"), value.clone());
        }
    }

    let mut request_metadata = HashMap::new();
    if let Some(req) = request {
        if !req.request_id.is_empty() {
            request_metadata.insert("request_id".to_string(), req.request_id.clone());
        }
        if let Some(trace_ctx) = req.trace.as_ref() {
            if !trace_ctx.trace_id.is_empty() {
                request_metadata.insert("trace_id".to_string(), trace_ctx.trace_id.clone());
            }
            if !trace_ctx.span_id.is_empty() {
                request_metadata.insert("span_id".to_string(), trace_ctx.span_id.clone());
            }
        }
    }

    let mut tags = HashMap::new();
    tags.insert("user_id".to_string(), user_id.to_string());
    tags.insert("push.message_type".to_string(), message_type.to_string());

    let mut context = HookContext::new(tenant_id.clone())
        .with_session(conversation_id.clone())
        .with_conversation_type(SESSION_TYPE_PUSH)
        .with_message_type(message_type.to_string())
        .with_sender(SENDER_PUSH_SERVER)
        .occurred_now();

    if let Some(req) = request {
        if let Some(trace_ctx) = req.trace.as_ref() {
            if !trace_ctx.trace_id.is_empty() {
                context = context.with_trace(trace_ctx.trace_id.clone());
            }
        }
    }

    context.tags = tags.clone();
    context.attributes = extra_metadata.clone();
    context.request_metadata = request_metadata;

    let mut draft = MessageDraft::new(payload);
    let message_id = Uuid::new_v4().to_string();
    draft.set_message_id(&message_id);
    draft.set_conversation_id(conversation_id.clone());
    if let Some(req) = request {
        if !req.request_id.is_empty() {
            draft.set_client_message_id(req.request_id.clone());
        }
    }
    draft.headers = options
        .map(|opts| opts.metadata.clone())
        .unwrap_or_default();
    draft.metadata = extra_metadata.clone();

    let envelope = PushHookEnvelope {
        context,
        message_id,
        conversation_id,
        message_type: message_type.to_string(),
        tenant_id,
    };

    (envelope, draft)
}
