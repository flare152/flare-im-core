use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use prost_types::Timestamp;
use tonic::IntoRequest;
use tonic::transport::{Channel, Endpoint};

use crate::error::{ErrorBuilder, ErrorCode, Result, from_rpc_status};
use flare_proto::common::Message as ProtoStorageMessage;
use flare_proto::common::{
    ActorContext as ProtoActorContext, ActorType as ProtoActorType,
    DeviceContext as ProtoDeviceContext, TraceContext as ProtoTraceContext,
};
use flare_proto::{
    HookExtensionClient, ProtoDeliveryHookRequest, ProtoDeliveryHookResponse,
    ProtoHookDeliveryEvent, ProtoHookInvocationContext, ProtoHookMessageDraft,
    ProtoHookMessageRecord, ProtoPostSendHookRequest, ProtoPreSendHookRequest,
    ProtoRecallHookRequest, ProtoRecallHookResponse, RequestContext as ProtoRequestContext,
    TenantContext as ProtoTenantContext,
};

use super::super::config::HookDefinition;
use super::super::types::{
    DeliveryEvent, DeliveryHook, HookContext, HookOutcome, MessageDraft, MessageRecord,
    PostSendHook, PreSendDecision, PreSendHook, RecallEvent, RecallHook,
};

#[derive(Clone)]
pub struct GrpcHookFactory;

impl GrpcHookFactory {
    pub fn new() -> Self {
        Self
    }

    fn build_channel(endpoint: &str) -> Result<Channel> {
        let endpoint = Endpoint::from_shared(endpoint.to_string()).map_err(|err| {
            ErrorBuilder::new(ErrorCode::ConfigurationError, "invalid gRPC hook endpoint")
                .details(err.to_string())
                .build_error()
        })?;
        Ok(endpoint.connect_lazy())
    }

    pub fn build_pre_send(
        &self,
        metadata: HashMap<String, String>,
        channel: Channel,
    ) -> Arc<dyn PreSendHook> {
        Arc::new(GrpcPreSendHook {
            channel,
            static_metadata: metadata,
        })
    }

    pub fn build_post_send(
        &self,
        metadata: HashMap<String, String>,
        channel: Channel,
    ) -> Arc<dyn PostSendHook> {
        Arc::new(GrpcPostSendHook {
            channel,
            static_metadata: metadata,
        })
    }

    pub fn build_delivery(
        &self,
        metadata: HashMap<String, String>,
        channel: Channel,
    ) -> Arc<dyn DeliveryHook> {
        Arc::new(GrpcDeliveryHook {
            channel,
            static_metadata: metadata,
        })
    }

    pub fn build_recall(
        &self,
        metadata: HashMap<String, String>,
        channel: Channel,
    ) -> Arc<dyn RecallHook> {
        Arc::new(GrpcRecallHook {
            channel,
            static_metadata: metadata,
        })
    }

    pub fn channel_for(&self, def: &HookDefinition) -> Result<Channel> {
        match &def.transport {
            super::super::config::HookTransportConfig::Grpc { endpoint, .. } => {
                Self::build_channel(endpoint)
            }
            _ => Err(
                ErrorBuilder::new(ErrorCode::ConfigurationError, "transport is not gRPC")
                    .details(format!("hook={}", def.name))
                    .build_error(),
            ),
        }
    }
}

#[derive(Clone)]
struct GrpcPreSendHook {
    channel: Channel,
    static_metadata: HashMap<String, String>,
}

#[async_trait]
impl PreSendHook for GrpcPreSendHook {
    async fn handle(&self, ctx: &HookContext, draft: &mut MessageDraft) -> PreSendDecision {
        let mut client = HookExtensionClient::new(self.channel.clone());
        let mut request = ProtoPreSendHookRequest::default();
        request.context = Some(build_context(ctx, &self.static_metadata));
        request.draft = Some(build_draft(draft));

        let response = client.invoke_pre_send(request.into_request()).await;
        match response {
            Ok(resp) => {
                let inner = resp.into_inner();
                if !inner.allow {
                    let status = inner.status.unwrap_or_default();
                    let err = from_rpc_status(&status);
                    return PreSendDecision::Reject { error: err };
                }
                if let Some(draft_resp) = inner.draft {
                    apply_draft(draft, draft_resp);
                }
                PreSendDecision::Continue
            }
            Err(status) => {
                let err = ErrorBuilder::new(ErrorCode::ServiceUnavailable, "pre-send hook failed")
                    .details(status.to_string())
                    .build_error();
                PreSendDecision::Reject { error: err }
            }
        }
    }
}

#[derive(Clone)]
struct GrpcPostSendHook {
    channel: Channel,
    static_metadata: HashMap<String, String>,
}

#[async_trait]
impl PostSendHook for GrpcPostSendHook {
    async fn handle(
        &self,
        ctx: &HookContext,
        record: &MessageRecord,
        draft: &MessageDraft,
    ) -> HookOutcome {
        let mut client = HookExtensionClient::new(self.channel.clone());
        let mut request = ProtoPostSendHookRequest::default();
        request.context = Some(build_context(ctx, &self.static_metadata));
        request.record = Some(build_record(record));
        request.draft = Some(build_draft(draft));

        match client.invoke_post_send(request).await {
            Ok(resp) => {
                let inner = resp.into_inner();
                if inner.success {
                    HookOutcome::Completed
                } else {
                    let status = inner.status.unwrap_or_default();
                    HookOutcome::Failed(from_rpc_status(&status))
                }
            }
            Err(status) => {
                let err = ErrorBuilder::new(ErrorCode::ServiceUnavailable, "post-send hook failed")
                    .details(status.to_string())
                    .build_error();
                HookOutcome::Failed(err)
            }
        }
    }
}

#[derive(Clone)]
struct GrpcDeliveryHook {
    channel: Channel,
    static_metadata: HashMap<String, String>,
}

#[async_trait]
impl DeliveryHook for GrpcDeliveryHook {
    async fn handle(&self, ctx: &HookContext, event: &DeliveryEvent) -> HookOutcome {
        let mut client = HookExtensionClient::new(self.channel.clone());
        let mut request = ProtoDeliveryHookRequest::default();
        request.context = Some(build_context(ctx, &self.static_metadata));
        request.event = Some(build_delivery_event(event));

        match client.notify_delivery(request).await {
            Ok(resp) => {
                let inner: ProtoDeliveryHookResponse = resp.into_inner();
                if inner.success {
                    HookOutcome::Completed
                } else {
                    let status = inner.status.unwrap_or_default();
                    HookOutcome::Failed(from_rpc_status(&status))
                }
            }
            Err(status) => {
                let err = ErrorBuilder::new(ErrorCode::ServiceUnavailable, "delivery hook failed")
                    .details(status.to_string())
                    .build_error();
                HookOutcome::Failed(err)
            }
        }
    }
}

#[derive(Clone)]
struct GrpcRecallHook {
    channel: Channel,
    static_metadata: HashMap<String, String>,
}

#[async_trait]
impl RecallHook for GrpcRecallHook {
    async fn handle(&self, ctx: &HookContext, event: &RecallEvent) -> HookOutcome {
        let mut client = HookExtensionClient::new(self.channel.clone());
        let mut request = ProtoRecallHookRequest::default();
        request.context = Some(build_context(ctx, &self.static_metadata));
        request.event = Some(build_recall_event(event));

        match client.notify_recall(request).await {
            Ok(resp) => {
                let inner: ProtoRecallHookResponse = resp.into_inner();
                if inner.allow {
                    HookOutcome::Completed
                } else {
                    let status = inner.status.unwrap_or_default();
                    HookOutcome::Failed(from_rpc_status(&status))
                }
            }
            Err(status) => {
                let err = ErrorBuilder::new(ErrorCode::ServiceUnavailable, "recall hook failed")
                    .details(status.to_string())
                    .build_error();
                HookOutcome::Failed(err)
            }
        }
    }
}

fn build_context(
    ctx: &HookContext,
    static_metadata: &HashMap<String, String>,
) -> ProtoHookInvocationContext {
    let corridor = ctx
        .attributes
        .get("corridor")
        .cloned()
        .or_else(|| ctx.session_type.clone())
        .unwrap_or_else(|| "messaging".to_string());

    let mut attributes = ctx.attributes.clone();
    for (key, value) in static_metadata {
        attributes
            .entry(key.clone())
            .or_insert_with(|| value.clone());
    }
    for (key, value) in &ctx.request_metadata {
        attributes
            .entry(format!("request.{key}"))
            .or_insert_with(|| value.clone());
    }

    ProtoHookInvocationContext {
        request_context: build_request_context(ctx),
        tenant: Some(build_tenant_context(ctx)),
        session_id: ctx.session_id.clone().unwrap_or_default(),
        session_type: ctx.session_type.clone().unwrap_or_default(),
        corridor,
        tags: ctx.tags.clone(),
        attributes,
    }
}

fn build_request_context(ctx: &HookContext) -> Option<ProtoRequestContext> {
    let mut has_context = false;

    let request_id = ctx
        .request_metadata
        .get("request_id")
        .cloned()
        .or_else(|| ctx.trace_id.clone());

    let trace = ctx.trace_id.as_ref().map(|trace_id| {
        has_context = true;
        ProtoTraceContext {
            trace_id: trace_id.clone(),
            span_id: ctx
                .request_metadata
                .get("span_id")
                .cloned()
                .unwrap_or_default(),
            parent_span_id: ctx
                .request_metadata
                .get("parent_span_id")
                .cloned()
                .unwrap_or_default(),
            sampled: ctx
                .request_metadata
                .get("trace_sampled")
                .cloned()
                .unwrap_or_default(),
            tags: ctx
                .request_metadata
                .iter()
                .filter_map(|(k, v)| {
                    if let Some(rest) = k.strip_prefix("trace.tag.") {
                        Some((rest.to_string(), v.clone()))
                    } else {
                        None
                    }
                })
                .collect(),
        }
    });

    let actor_id = ctx
        .sender_id
        .clone()
        .or_else(|| ctx.request_metadata.get("actor_id").cloned());

    let actor = actor_id.map(|id| {
        has_context = true;
        let roles = ctx
            .attributes
            .get("actor_roles")
            .map(|raw| {
                raw.split(',')
                    .filter_map(|r| {
                        let trimmed = r.trim();
                        (!trimmed.is_empty()).then(|| trimmed.to_string())
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let actor_type = ctx
            .attributes
            .get("actor_type")
            .and_then(|v| match v.to_ascii_lowercase().as_str() {
                "service" => Some(ProtoActorType::Service),
                "tenant_admin" | "tenant-admin" => Some(ProtoActorType::TenantAdmin),
                "system" => Some(ProtoActorType::System),
                "guest" => Some(ProtoActorType::Guest),
                "user" => Some(ProtoActorType::User),
                "unspecified" => Some(ProtoActorType::Unspecified),
                _ => None,
            })
            .unwrap_or(ProtoActorType::User);

        ProtoActorContext {
            actor_id: id,
            r#type: actor_type as i32,
            roles,
            attributes: ctx
                .attributes
                .iter()
                .filter_map(|(k, v)| {
                    if let Some(rest) = k.strip_prefix("actor.attr.") {
                        Some((rest.to_string(), v.clone()))
                    } else {
                        None
                    }
                })
                .collect(),
        }
    });

    let device = {
        let device_id = ctx
            .request_metadata
            .get("device_id")
            .cloned()
            .or_else(|| ctx.attributes.get("device_id").cloned());
        if device_id.is_some() {
            has_context = true;
        }
        device_id.map(|id| ProtoDeviceContext {
            device_id: id,
            platform: ctx
                .request_metadata
                .get("device_platform")
                .cloned()
                .or_else(|| ctx.attributes.get("device_platform").cloned())
                .unwrap_or_default(),
            model: ctx
                .request_metadata
                .get("device_model")
                .cloned()
                .unwrap_or_default(),
            os_version: ctx
                .request_metadata
                .get("os_version")
                .cloned()
                .unwrap_or_else(|| {
                    ctx.attributes
                        .get("os_version")
                        .cloned()
                        .unwrap_or_default()
                }),
            app_version: ctx
                .request_metadata
                .get("app_version")
                .cloned()
                .unwrap_or_else(|| {
                    ctx.attributes
                        .get("app_version")
                        .cloned()
                        .unwrap_or_default()
                }),
            locale: ctx
                .request_metadata
                .get("locale")
                .cloned()
                .unwrap_or_default(),
            timezone: ctx
                .request_metadata
                .get("timezone")
                .cloned()
                .unwrap_or_default(),
            ip_address: ctx
                .request_metadata
                .get("ip_address")
                .cloned()
                .unwrap_or_default(),
            attributes: ctx
                .request_metadata
                .iter()
                .filter_map(|(k, v)| {
                    if let Some(rest) = k.strip_prefix("device.attr.") {
                        Some((rest.to_string(), v.clone()))
                    } else {
                        None
                    }
                })
                .collect(),
            priority: 0,  // 【新增】默认为 Unspecified
            token_version: 0,  // 【新增】默认为 0
            connection_quality: None,  // 【新增】默认为 None
        })
    };

    if !has_context && request_id.is_none() {
        return None;
    }

    Some(ProtoRequestContext {
        request_id: request_id.unwrap_or_default(),
        trace,
        actor,
        device,
        channel: ctx
            .attributes
            .get("channel")
            .cloned()
            .or_else(|| ctx.request_metadata.get("channel").cloned())
            .unwrap_or_else(|| "grpc".to_string()),
        user_agent: ctx
            .request_metadata
            .get("user_agent")
            .cloned()
            .unwrap_or_default(),
        attributes: ctx
            .request_metadata
            .iter()
            .filter_map(|(k, v)| {
                if let Some(rest) = k.strip_prefix("request.attr.") {
                    Some((rest.to_string(), v.clone()))
                } else {
                    None
                }
            })
            .collect(),
    })
}

fn build_tenant_context(ctx: &HookContext) -> ProtoTenantContext {
    let business_type = ctx
        .attributes
        .get("tenant_business_type")
        .cloned()
        .unwrap_or_default();
    let environment = ctx
        .attributes
        .get("tenant_environment")
        .cloned()
        .unwrap_or_default();
    let organization_id = ctx
        .attributes
        .get("tenant_organization_id")
        .cloned()
        .unwrap_or_default();

    ProtoTenantContext {
        tenant_id: ctx.tenant_id.clone(),
        business_type,
        environment,
        organization_id,
        labels: HashMap::new(),
        attributes: HashMap::new(),
    }
}

fn build_draft(draft: &MessageDraft) -> ProtoHookMessageDraft {
    ProtoHookMessageDraft {
        message_id: draft.message_id.clone().unwrap_or_default(),
        client_message_id: draft.client_message_id.clone().unwrap_or_default(),
        conversation_id: draft.conversation_id.clone().unwrap_or_default(),
        payload: draft.payload.clone(),
        headers: draft.headers.clone(),
        metadata: draft.metadata.clone(),
    }
}

fn apply_draft(target: &mut MessageDraft, source: ProtoHookMessageDraft) {
    if !source.message_id.is_empty() {
        target.message_id = Some(source.message_id);
    }
    if !source.client_message_id.is_empty() {
        target.client_message_id = Some(source.client_message_id);
    }
    if !source.conversation_id.is_empty() {
        target.conversation_id = Some(source.conversation_id);
    }
    target.payload = source.payload;
    target.headers = source.headers;
    target.metadata = source.metadata;
}

fn build_record(record: &MessageRecord) -> ProtoHookMessageRecord {
    let persisted_ts = system_time_to_timestamp(record.persisted_at);

    let mut message = ProtoStorageMessage::default();
    message.id = record.message_id.clone();
    message.session_id = record.conversation_id.clone();
    message.sender_id = record.sender_id.clone();
    message.session_type = record
        .session_type
        .as_deref()
        .map(|t| match t.to_ascii_lowercase().as_str() {
            "single" | "session_type_single" | "1" => flare_proto::common::SessionType::Single as i32,
            "group" | "session_type_group" | "2" => flare_proto::common::SessionType::Group as i32,
            "channel" | "session_type_channel" | "3" => flare_proto::common::SessionType::Channel as i32,
            _ => flare_proto::common::SessionType::Unspecified as i32,
        })
        .unwrap_or(flare_proto::common::SessionType::Unspecified as i32);
    message.extra = record.metadata.clone();
    message.timestamp = Some(persisted_ts.clone());
    message.message_type = record
        .message_type
        .as_deref()
        .map(|kind| match kind.to_ascii_lowercase().as_str() {
            "text" | "message_type_text" => flare_proto::common::MessageType::Text as i32,
            "image" => flare_proto::common::MessageType::Image as i32,
            "video" => flare_proto::common::MessageType::Video as i32,
            "audio" => flare_proto::common::MessageType::Audio as i32,
            "file" => flare_proto::common::MessageType::File as i32,
            "location" => flare_proto::common::MessageType::Location as i32,
            "card" => flare_proto::common::MessageType::Card as i32,
            "notification" => flare_proto::common::MessageType::Notification as i32,
            "binary" | "attachment" | "message_type_binary" => flare_proto::common::MessageType::Custom as i32, // 二进制消息映射到 Custom
            "custom" | "message_type_custom" => flare_proto::common::MessageType::Custom as i32,
            _ => flare_proto::common::MessageType::Unspecified as i32,
        })
        .unwrap_or(flare_proto::common::MessageType::Unspecified as i32);
    if let Some(message_type) = &record.message_type {
        message
            .extra
            .entry("message_type".into())
            .or_insert_with(|| message_type.clone());
    }

    if let Some(client_id) = record.client_message_id.as_ref() {
        message
            .extra
            .entry("client_message_id".into())
            .or_insert_with(|| client_id.clone());
    }

    ProtoHookMessageRecord {
        message: Some(message),
        persisted_at: Some(persisted_ts),
        metadata: record.metadata.clone(),
    }
}

fn build_delivery_event(event: &DeliveryEvent) -> ProtoHookDeliveryEvent {
    ProtoHookDeliveryEvent {
        message_id: event.message_id.clone(),
        user_id: event.user_id.clone(),
        channel: event.channel.clone(),
        delivered_at: Some(system_time_to_timestamp(event.delivered_at)),
        metadata: event.metadata.clone(),
    }
}

fn build_recall_event(event: &RecallEvent) -> flare_proto::ProtoHookRecallEvent {
    flare_proto::ProtoHookRecallEvent {
        message_id: event.message_id.clone(),
        operator_id: event.operator_id.clone(),
        recalled_at: Some(system_time_to_timestamp(event.recalled_at)),
        metadata: event.metadata.clone(),
    }
}

fn system_time_to_timestamp(time: SystemTime) -> Timestamp {
    let duration = time
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| std::time::Duration::from_secs(0));
    Timestamp {
        seconds: duration.as_secs() as i64,
        nanos: duration.subsec_nanos() as i32,
    }
}
