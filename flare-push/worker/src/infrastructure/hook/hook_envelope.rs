//! Hook 信封构建工具

use std::collections::HashMap;
use std::time::SystemTime;

use flare_im_core::hooks::DeliveryEvent;
use flare_im_core::hooks::hook_context_data::{HookContextData, set_hook_context_data};
use flare_server_core::context::Context;

use crate::domain::model::{PushDispatchTask, RequestMetadata};

const FALLBACK_MESSAGE_TYPE: &str = "push_dispatch";
const SESSION_TYPE_PUSH: &str = "push";

pub fn build_delivery_context(default_tenant_id: &str, task: &PushDispatchTask) -> Context {
    let tenant_id = task
        .tenant_id
        .clone()
        .unwrap_or_else(|| default_tenant_id.to_string());
    let message_type = if task.message_type.trim().is_empty() {
        FALLBACK_MESSAGE_TYPE.to_string()
    } else {
        task.message_type.clone()
    };

    let mut tags = HashMap::new();
    tags.insert("user_id".to_string(), task.user_id.clone());
    tags.insert("push.message_type".to_string(), message_type.clone());
    for (key, value) in &task.headers {
        tags.insert(format!("header.{key}"), value.clone());
    }

    let mut attributes = task.metadata.clone();
    let mut request_metadata = HashMap::new();
    if let Some(context) = task.context.as_ref() {
        merge_request_metadata(context, &mut tags, &mut attributes, &mut request_metadata);
    }

    if task.message_id.is_empty() {
        attributes.insert(
            "idempotency_key".to_string(),
            format!("dispatch:{}:{}", message_type, task.user_id),
        );
    } else {
        attributes.insert(
            "idempotency_key".to_string(),
            format!("dispatch:{}:{}", task.message_id, task.user_id),
        );
    }

    // 创建 Context
    let request_id = task
        .context
        .as_ref()
        .map(|ctx| ctx.request_id.clone())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    
    let mut ctx = Context::with_request_id(request_id);
    
    // 设置租户ID
    if !tenant_id.is_empty() {
        ctx = ctx.with_tenant_id(tenant_id);
    }
    
    // 设置 trace_id
    if let Some(trace_id) = task.context.as_ref().and_then(|ctx| ctx.trace_id.clone()) {
        ctx = ctx.with_trace_id(trace_id);
    }
    
    // 设置会话ID
    let conversation_id = format!("{SESSION_TYPE_PUSH}:{}", task.user_id);
    ctx = ctx.with_session_id(conversation_id.clone());
    
    // 创建 HookContextData
    let hook_data = HookContextData::new()
        .with_conversation_id(conversation_id)
        .with_conversation_type(SESSION_TYPE_PUSH)
        .with_message_type(message_type)
        .with_tags(tags)
        .with_attributes(attributes)
        .with_request_metadata(request_metadata)
        .occurred_now();
    
    // 将 HookContextData 存储到 Context
    set_hook_context_data(ctx, hook_data)
}

pub fn build_delivery_event(task: &PushDispatchTask, channel: &str) -> DeliveryEvent {
    let mut metadata = task.metadata.clone();
    metadata.insert("ack_type".to_string(), channel.to_string());

    DeliveryEvent {
        message_id: if task.message_id.is_empty() {
            format!("{}:{}", task.user_id, channel)
        } else {
            task.message_id.clone()
        },
        user_id: task.user_id.clone(),
        channel: channel.to_string(),
        delivered_at: SystemTime::now(),
        metadata,
    }
}

fn merge_request_metadata(
    context: &RequestMetadata,
    tags: &mut HashMap<String, String>,
    attributes: &mut HashMap<String, String>,
    request_metadata: &mut HashMap<String, String>,
) {
    if !context.request_id.is_empty() {
        request_metadata.insert("request_id".to_string(), context.request_id.clone());
    }
    if let Some(trace_id) = context.trace_id.as_ref() {
        tags.insert("ctx.trace_id".to_string(), trace_id.clone());
        attributes.insert("ctx.trace_id".to_string(), trace_id.clone());
        request_metadata.insert("trace_id".to_string(), trace_id.clone());
    }
    if let Some(span_id) = context.span_id.as_ref() {
        tags.insert("ctx.span_id".to_string(), span_id.clone());
        attributes.insert("ctx.span_id".to_string(), span_id.clone());
        request_metadata.insert("span_id".to_string(), span_id.clone());
    }
    if let Some(client_ip) = context.client_ip.as_ref() {
        attributes.insert("client_ip".to_string(), client_ip.clone());
    }
    if let Some(user_agent) = context.user_agent.as_ref() {
        attributes.insert("user_agent".to_string(), user_agent.clone());
    }
}
