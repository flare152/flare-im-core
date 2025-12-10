//! # 类型转换辅助模块
//!
//! 提供 flare-im-core 类型和 protobuf 类型之间的转换

use prost_types::Timestamp;
use std::time::{SystemTime, UNIX_EPOCH};

use flare_proto::hooks::{
    HookInvocationContext, HookMessageDraft, HookMessageRecord, HookDeliveryEvent,
    HookRecallEvent, PreSendHookResponse, RecallHookResponse,
};
use flare_proto::common::{RequestContext, TenantContext};
use flare_im_core::{
    HookContext, MessageDraft, MessageRecord, DeliveryEvent, RecallEvent,
    PreSendDecision,
};

/// 将 HookContext 转换为 HookInvocationContext
pub fn hook_context_to_proto(ctx: &HookContext) -> HookInvocationContext {
    HookInvocationContext {
        request_context: Some(RequestContext {
            request_id: ctx.trace_id.clone().unwrap_or_default(),
            trace: None,
            actor: None,
            device: None,
            channel: String::new(),
            user_agent: String::new(),
            attributes: std::collections::HashMap::new(),
        }),
        tenant: Some(TenantContext {
            tenant_id: ctx.tenant_id.clone(),
            business_type: String::new(),
            environment: String::new(),
            organization_id: String::new(),
            labels: std::collections::HashMap::new(),
            attributes: std::collections::HashMap::new(),
        }),
        session_id: ctx.session_id.clone().unwrap_or_default(),
        session_type: ctx.session_type.clone().unwrap_or_default(),
        corridor: ctx.attributes.get("corridor").cloned().unwrap_or_else(|| "messaging".to_string()),
        tags: ctx.tags.clone(),
        attributes: ctx.attributes.clone(),
    }
}

/// 将 MessageDraft 转换为 HookMessageDraft
pub fn message_draft_to_proto(draft: &MessageDraft) -> HookMessageDraft {
    HookMessageDraft {
        message_id: draft.message_id.clone().unwrap_or_default(),
        client_message_id: draft.client_message_id.clone().unwrap_or_default(),
        conversation_id: draft.conversation_id.clone().unwrap_or_default(),
        payload: draft.payload.clone(),
        headers: draft.headers.clone(),
        metadata: draft.metadata.clone(),
    }
}

/// 将 HookMessageDraft 转换为 MessageDraft
pub fn proto_to_message_draft(proto: &HookMessageDraft) -> MessageDraft {
    let mut draft = MessageDraft::new(proto.payload.clone());
    if !proto.message_id.is_empty() {
        draft.set_message_id(proto.message_id.clone());
    }
    if !proto.client_message_id.is_empty() {
        draft.set_client_message_id(proto.client_message_id.clone());
    }
    if !proto.conversation_id.is_empty() {
        draft.set_conversation_id(proto.conversation_id.clone());
    }
    draft.headers = proto.headers.clone();
    draft.metadata = proto.metadata.clone();
    draft
}

/// 将 MessageRecord 转换为 HookMessageRecord
pub fn message_record_to_proto(record: &MessageRecord) -> HookMessageRecord {
    // 将 MessageRecord 转换为 protobuf Message
    let ts = system_time_to_timestamp(record.persisted_at);
    let proto_message = flare_proto::common::Message {
        id: record.message_id.clone(),
        session_id: record.conversation_id.clone(),
        client_msg_id: record.client_message_id.clone().unwrap_or_default(),
        sender_id: record.sender_id.clone(),
        source: 1,
        seq: 0,
        timestamp: Some(ts.clone()),
        session_type: record
            .session_type
            .as_deref()
            .map(|t| match t.to_ascii_lowercase().as_str() {
                "single" | "1" => 1,
                "group" | "2" => 2,
                "channel" | "3" => 3,
                _ => 0,
            })
            .unwrap_or(0),
        message_type: 0,
        business_type: String::new(),
        content: None,
        content_type: 1,
        attachments: vec![],
        extra: std::collections::HashMap::new(),
        attributes: std::collections::HashMap::new(),
        status: 1,
        is_recalled: false,
        recalled_at: None,
        recall_reason: String::new(),
        is_burn_after_read: false,
        burn_after_seconds: 0,
        timeline: Some(flare_proto::common::MessageTimeline {
            created_at: None,
            persisted_at: Some(ts),
            delivered_at: None,
            read_at: None,
        }),
        visibility: std::collections::HashMap::new(),
        read_by: vec![],
        reactions: vec![],
        edit_history: vec![],
        tenant: None,
        audit: None,
        tags: vec![],
        offline_push_info: None,
        extensions: vec![],
    };

    HookMessageRecord {
        message: Some(proto_message),
        persisted_at: Some(system_time_to_timestamp(record.persisted_at)),
        metadata: record.metadata.clone(),
    }
}

/// 将 DeliveryEvent 转换为 HookDeliveryEvent
pub fn delivery_event_to_proto(event: &DeliveryEvent) -> HookDeliveryEvent {
    HookDeliveryEvent {
        message_id: event.message_id.clone(),
        user_id: event.user_id.clone(),
        channel: event.channel.clone(),
        delivered_at: Some(system_time_to_timestamp(event.delivered_at)),
        metadata: event.metadata.clone(),
    }
}

/// 将 RecallEvent 转换为 HookRecallEvent
pub fn recall_event_to_proto(event: &RecallEvent) -> HookRecallEvent {
    HookRecallEvent {
        message_id: event.message_id.clone(),
        operator_id: event.operator_id.clone(),
        recalled_at: Some(system_time_to_timestamp(event.recalled_at)),
        metadata: event.metadata.clone(),
    }
}

/// 将 PreSendHookResponse 转换为 PreSendDecision
pub fn proto_to_pre_send_decision(
    response: &PreSendHookResponse,
    draft: &mut MessageDraft,
) -> PreSendDecision {
    if response.allow {
        // 如果允许发送，更新 draft（如果有修改）
        if let Some(ref updated_draft) = response.draft {
            *draft = proto_to_message_draft(updated_draft);
        }
        PreSendDecision::Continue
    } else {
        // 如果不允许发送，从 status 构建错误
        use flare_im_core::error::{ErrorBuilder, ErrorCode};
        let error = if let Some(ref status) = response.status {
            let code = ErrorCode::from_u32(status.code as u32).unwrap_or(ErrorCode::GeneralError);
            ErrorBuilder::new(code, &status.message)
                .build_error()
        } else {
            ErrorBuilder::new(
                ErrorCode::PermissionDenied,
                "Hook rejected the request",
            )
            .build_error()
        };
        PreSendDecision::Reject { error }
    }
}

/// 将 RecallHookResponse 转换为 PreSendDecision
pub fn proto_to_recall_decision(response: &RecallHookResponse) -> PreSendDecision {
    if response.allow {
        PreSendDecision::Continue
    } else {
        use flare_im_core::error::{ErrorBuilder, ErrorCode};
        let error = if let Some(ref status) = response.status {
            let code = ErrorCode::from_u32(status.code as u32).unwrap_or(ErrorCode::GeneralError);
            ErrorBuilder::new(code, &status.message)
                .build_error()
        } else {
            ErrorBuilder::new(
                ErrorCode::PermissionDenied,
                "Hook rejected the recall request",
            )
            .build_error()
        };
        PreSendDecision::Reject { error }
    }
}

/// 将 SystemTime 转换为 Timestamp
pub fn system_time_to_timestamp(time: SystemTime) -> Timestamp {
    time.duration_since(UNIX_EPOCH)
        .map(|d| Timestamp {
            seconds: d.as_secs() as i64,
            nanos: d.subsec_nanos() as i32,
        })
        .unwrap_or_else(|_| Timestamp {
            seconds: 0,
            nanos: 0,
        })
}

/// 将 Timestamp 转换为 SystemTime
pub fn timestamp_to_system_time(ts: &Timestamp) -> SystemTime {
    UNIX_EPOCH + std::time::Duration::from_secs(ts.seconds as u64)
        + std::time::Duration::from_nanos(ts.nanos as u64)
}
