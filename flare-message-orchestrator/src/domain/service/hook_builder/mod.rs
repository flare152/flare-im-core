use std::collections::HashMap;
use std::time::SystemTime;

use flare_im_core::hooks::{HookContext, MessageDraft, MessageRecord};
use flare_proto::common::{RequestContext, TenantContext};
use flare_proto::common::Message;
use flare_proto::storage::StoreMessageRequest;
use serde_json::json;

use crate::domain::model::MessageSubmission;

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
        trace,
        actor: _,
        device,
        channel: _,
        user_agent,
        attributes,
    }) = request.context.as_ref()
    {
        ctx.request_metadata
            .insert("request_id".into(), request_id.clone());
        
        if let Some(trace_ctx) = trace.as_ref() {
            ctx.request_metadata
                .insert("span_id".into(), trace_ctx.span_id.clone());
            if !trace_ctx.trace_id.is_empty() {
                ctx.trace_id = Some(trace_ctx.trace_id.clone());
            }
            // 将 trace tags 添加到 ctx.tags
            for (k, v) in &trace_ctx.tags {
                ctx.tags.insert(k.clone(), v.clone());
            }
        }
        
        if let Some(device_ctx) = device.as_ref() {
            ctx.request_metadata
                .insert("client_ip".into(), device_ctx.ip_address.clone());
        }
        
        if !user_agent.is_empty() {
            ctx.request_metadata
                .insert("user_agent".into(), user_agent.clone());
        }
        
        // 将 attributes 添加到 ctx.tags
        for (k, v) in attributes {
            ctx.tags.insert(k.clone(), v.clone());
        }
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
        // session_type 是 i32 枚举，转换为字符串
        let session_type_str = match flare_proto::common::SessionType::try_from(message.session_type) {
            Ok(flare_proto::common::SessionType::Single) => "single".to_string(),
            Ok(flare_proto::common::SessionType::Group) => "group".to_string(),
            Ok(flare_proto::common::SessionType::Channel) => "channel".to_string(),
            _ => "unknown".to_string(),
        };
        ctx.session_type = non_empty(session_type_str.clone());
        ctx.message_type = Some(message_type_label.to_string());

        ctx.attributes
            .entry("business_type".into())
            .or_insert(message.business_type.clone());
        ctx.attributes
            .entry("session_type".into())
            .or_insert(session_type_str.clone());
        
        // 从 session_id 提取接收者信息
        let session_parts: Vec<&str> = message.session_id.split(':').collect();
        let receiver_list = if session_parts.len() >= 3 && session_parts.first() == Some(&"single") {
            session_parts[1..].iter()
                .filter(|&id| id != &message.sender_id)
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        
        ctx.attributes
            .entry("receiver_id".into())
            .or_insert(receiver_list.first().cloned().unwrap_or_default());
        if !receiver_list.is_empty() {
            ctx.attributes
                .entry("receiver_ids".into())
                .or_insert(receiver_list.join(","));
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

pub fn build_draft_from_request(request: &StoreMessageRequest) -> anyhow::Result<MessageDraft> {
    let message = request
        .message
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("StoreMessageRequest.message must be set"))?;

    // MessageDraft::new 需要 Vec<u8>，但 message.content 是 Option<MessageContent>
    // 使用 prost 序列化 MessageContent，或使用空向量
    use prost::Message as ProstMessage;
    let content_bytes = message.content.as_ref()
        .map(|c| {
            let mut buf = Vec::new();
            c.encode(&mut buf).unwrap_or_default();
            buf
        })
        .unwrap_or_default();
    let mut draft = MessageDraft::new(content_bytes);
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
    
    // session_type 是 i32 枚举，需要转换为字符串
    let session_type_str = match flare_proto::common::SessionType::try_from(message.session_type) {
        Ok(flare_proto::common::SessionType::Single) => "single".to_string(),
        Ok(flare_proto::common::SessionType::Group) => "group".to_string(),
        Ok(flare_proto::common::SessionType::Channel) => "channel".to_string(),
        _ => "unknown".to_string(),
    };
    metadata
        .entry("session_type".into())
        .or_insert(session_type_str);
    metadata
        .entry("message_type".into())
        .or_insert(message_type_label.to_string());
    // content_type 从 MessageContent 推断
    let content_type_label = message.content.as_ref()
        .map(|c| match &c.content {
            Some(flare_proto::common::message_content::Content::Text(_)) => "text",
            Some(flare_proto::common::message_content::Content::Image(_)) => "image",
            Some(flare_proto::common::message_content::Content::Video(_)) => "video",
            Some(flare_proto::common::message_content::Content::Audio(_)) => "audio",
            Some(flare_proto::common::message_content::Content::File(_)) => "file",
            Some(flare_proto::common::message_content::Content::Location(_)) => "location",
            Some(flare_proto::common::message_content::Content::Card(_)) => "card",
            Some(flare_proto::common::message_content::Content::Notification(_)) => "notification",
            Some(flare_proto::common::message_content::Content::Custom(_)) => "custom",
            Some(flare_proto::common::message_content::Content::Forward(_)) => "forward",
            Some(flare_proto::common::message_content::Content::Typing(_)) => "typing",
            // 富交互类型（注意：Vote、Task、Schedule、Announcement 在新版 protobuf 中已移除）
            Some(flare_proto::common::message_content::Content::SystemEvent(_)) => "system_event",
            Some(flare_proto::common::message_content::Content::Quote(_)) => "quote",
            Some(flare_proto::common::message_content::Content::LinkCard(_)) => "link_card",
            None => "unspecified",
        })
        .unwrap_or("unspecified");
    metadata
        .entry("content_type".into())
        .or_insert(content_type_label.to_string());
    metadata
        .entry("sender_id".into())
        .or_insert(message.sender_id.clone());
    
    // 从 session_id 提取接收者信息（新版 Message 已移除 receiver_id 和 receiver_ids 字段）
    let session_parts: Vec<&str> = message.session_id.split(':').collect();
    let receiver_list = if session_parts.len() >= 3 && session_parts.first() == Some(&"single") {
        session_parts[1..].iter()
            .filter(|&id| id != &message.sender_id)
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    
    metadata
        .entry("receiver_id".into())
        .or_insert(receiver_list.first().cloned().unwrap_or_default());
    metadata
        .entry("receiver_ids".into())
        .or_insert(if receiver_list.is_empty() {
            String::new()
        } else {
            receiver_list.join(",")
        });
    draft.metadata = metadata;

    draft.extra("session_id", json!(request.session_id));
    draft.extra("sync", json!(request.sync));
    draft.extra("receiver_ids", json!(receiver_list));

    // request.context 是 RequestContext，我们需要从中提取信息构建 JSON
    if let Some(request_ctx) = request.context.as_ref() {
        let mut request_context_json = json!({
            "request_id": request_ctx.request_id,
        });
        
        if let Some(trace_ctx) = request_ctx.trace.as_ref() {
            if !trace_ctx.trace_id.is_empty() {
                request_context_json["trace_id"] = json!(trace_ctx.trace_id);
            }
            if !trace_ctx.span_id.is_empty() {
                request_context_json["span_id"] = json!(trace_ctx.span_id);
            }
        }
        
        // 将 attributes 添加到 JSON
        if !request_ctx.attributes.is_empty() {
            request_context_json["attributes"] = json!(request_ctx.attributes);
        }
        
        draft.extra("request_context", request_context_json);
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

    Ok(draft)
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

        // message.content 是 MessageContent，需要根据 draft.payload 构建
        // message.extra 用于存储扩展信息
        message.extra = draft.metadata.clone();
        // message_type 从 extra 中的 message_type 获取，如果没有则使用默认值
        if let Some(label) = message.extra.get("message_type") {
            // 根据 label 设置 message_type 枚举
            use flare_proto::common::MessageType;
            message.message_type = match label.as_str() {
                "text" => MessageType::Text as i32,
                "image" => MessageType::Image as i32,
                "video" => MessageType::Video as i32,
                "audio" => MessageType::Audio as i32,
                "file" => MessageType::File as i32,
                "location" => MessageType::Location as i32,
                "card" => MessageType::Card as i32,
                "notification" => MessageType::Notification as i32,
                "custom" => MessageType::Custom as i32,
                _ => MessageType::Unspecified as i32,
            };
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
    let message = &submission.message;
    let mut metadata: HashMap<String, String> = message.extra.clone();

    metadata.insert("business_type".into(), message.business_type.clone());
    // session_type 是 i32 枚举，转换为字符串
    let session_type_str = match flare_proto::common::SessionType::try_from(message.session_type) {
        Ok(flare_proto::common::SessionType::Single) => "single",
        Ok(flare_proto::common::SessionType::Group) => "group",
        Ok(flare_proto::common::SessionType::Channel) => "channel",
        _ => "unknown",
    };
    metadata.insert("session_type".into(), session_type_str.to_string());
    // 推断 content_type
    let content_type = message.content.as_ref()
        .map(|c| match &c.content {
            Some(flare_proto::common::message_content::Content::Text(_)) => "text/plain",
            Some(flare_proto::common::message_content::Content::Image(_)) => "image/*",
            Some(flare_proto::common::message_content::Content::Video(_)) => "video/*",
            Some(flare_proto::common::message_content::Content::Audio(_)) => "audio/*",
            Some(flare_proto::common::message_content::Content::File(_)) => "application/octet-stream",
            Some(flare_proto::common::message_content::Content::Location(_)) => "location",
            Some(flare_proto::common::message_content::Content::Card(_)) => "card",
            Some(flare_proto::common::message_content::Content::Notification(_)) => "notification",
            Some(flare_proto::common::message_content::Content::Custom(_)) => "application/custom",
            Some(flare_proto::common::message_content::Content::Forward(_)) => "forward",
            Some(flare_proto::common::message_content::Content::Typing(_)) => "typing",
            // 富交互类型（注意：这些类型在新版 protobuf 中可能已移除）
            Some(flare_proto::common::message_content::Content::SystemEvent(_)) => "system_event",
            Some(flare_proto::common::message_content::Content::Quote(_)) => "quote",
            Some(flare_proto::common::message_content::Content::LinkCard(_)) => "link_card",
            None => "application/unknown",
        })
        .unwrap_or("application/unknown");
    metadata.insert("content_type".into(), content_type.to_string());

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
        message_id: message.id.clone(),
        client_message_id: None,
        conversation_id: message.session_id.clone(),
        sender_id: message.sender_id.clone(),
        session_type: Some(session_type_str.to_string()),
        message_type: metadata.get("content_type").cloned(),
        persisted_at: SystemTime::now(),
        metadata,
    }
}

pub fn draft_from_submission(submission: &MessageSubmission) -> anyhow::Result<MessageDraft> {
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
    use std::convert::TryFrom;
    use flare_proto::common::MessageType;
    
    // 优先从 extra 中获取 message_type 标签
    if let Some(label) = message.extra.get("message_type") {
        return match label.as_str() {
            "text" | "text/plain" => "text",
            "binary" => "binary",
            "json" => "json",
            "image" => "image",
            "video" => "video",
            "audio" => "audio",
            "file" => "file",
            "sticker" => "sticker",
            "location" => "location",
            "card" => "card",
            "command" => "command",
            "event" => "event",
            "system" => "system",
            _ => "custom",
        };
    }
    
    // 从 MessageType 枚举推断（支持所有消息类型）
    match MessageType::try_from(message.message_type) {
        // 基础消息类型
        Ok(MessageType::Text) => "text",
        Ok(MessageType::Image) => "image",
        Ok(MessageType::Video) => "video",
        Ok(MessageType::Audio) => "audio",
        Ok(MessageType::File) => "file",
        Ok(MessageType::Location) => "location",
        Ok(MessageType::Card) => "card",
        Ok(MessageType::Custom) => "custom",
        Ok(MessageType::Notification) => "notification",
        // 功能消息类型
        Ok(MessageType::Typing) => "typing",
        Ok(MessageType::Recall) => "recall",
        Ok(MessageType::Read) => "read",
        Ok(MessageType::Forward) => "forward",
        // 业务扩展消息类型
        Ok(MessageType::Vote) => "vote",
        Ok(MessageType::Task) => "task",
        Ok(MessageType::Schedule) => "schedule",
        Ok(MessageType::Announcement) => "announcement",
        // 扩展消息类型
        Ok(MessageType::Quote) => "quote",
        Ok(MessageType::LinkCard) => "link_card",
        Ok(MessageType::MergeForward) => "merge_forward",
        Ok(MessageType::MiniProgram) => "mini_program",
        // 系统消息类型
        Ok(MessageType::SystemEvent) => "system_event",
        // 未知或错误情况
        Ok(MessageType::Unspecified) | Err(_) => {
            // 从 MessageContent 推断类型
            if let Some(content) = message.content.as_ref() {
                match &content.content {
                    Some(flare_proto::common::message_content::Content::Text(_)) => "text",
                    Some(flare_proto::common::message_content::Content::Image(_)) => "image",
                    Some(flare_proto::common::message_content::Content::Video(_)) => "video",
                    Some(flare_proto::common::message_content::Content::Audio(_)) => "audio",
                    Some(flare_proto::common::message_content::Content::File(_)) => "file",
                    Some(flare_proto::common::message_content::Content::Location(_)) => "location",
                    Some(flare_proto::common::message_content::Content::Card(_)) => "card",
                    Some(flare_proto::common::message_content::Content::Notification(_)) => "notification",
                    Some(flare_proto::common::message_content::Content::Custom(_)) => "custom",
                    Some(flare_proto::common::message_content::Content::Forward(_)) => "forward",
                    Some(flare_proto::common::message_content::Content::Typing(_)) => "typing",
                    Some(flare_proto::common::message_content::Content::SystemEvent(_)) => "system_event",
                    Some(flare_proto::common::message_content::Content::Quote(_)) => "quote",
                    Some(flare_proto::common::message_content::Content::LinkCard(_)) => "link_card",
                    None => "unknown",
                }
            } else {
                "unknown"
            }
        }
    }
}

#[allow(dead_code)]
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
