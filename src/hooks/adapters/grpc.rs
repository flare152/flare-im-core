use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use prost_types::Timestamp;
use tonic::IntoRequest;
use tonic::transport::{Channel, Endpoint};

use crate::error::{ErrorBuilder, ErrorCode, Result, from_rpc_status};
use flare_proto::{
    HookExtensionClient, ProtoDeliveryHookRequest, ProtoDeliveryHookResponse,
    ProtoHookDeliveryEvent, ProtoHookInvocationContext, ProtoHookMessageDraft,
    ProtoHookMessageRecord, ProtoPostSendHookRequest, ProtoPreSendHookRequest,
    ProtoRecallHookRequest, ProtoRecallHookResponse,
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
                if inner.success {
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
    let mut attributes = ctx.attributes.clone();
    for (key, value) in static_metadata {
        attributes
            .entry(key.clone())
            .or_insert_with(|| value.clone());
    }

    ProtoHookInvocationContext {
        tenant_id: ctx.tenant_id.clone(),
        session_id: ctx.session_id.clone().unwrap_or_default(),
        session_type: ctx.session_type.clone().unwrap_or_default(),
        message_type: ctx.message_type.clone().unwrap_or_default(),
        sender_id: ctx.sender_id.clone().unwrap_or_default(),
        trace_id: ctx.trace_id.clone().unwrap_or_default(),
        tags: ctx.tags.clone(),
        request_context: None,
        tenant: None,
        attributes,
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
    ProtoHookMessageRecord {
        message_id: record.message_id.clone(),
        client_message_id: record.client_message_id.clone().unwrap_or_default(),
        conversation_id: record.conversation_id.clone(),
        sender_id: record.sender_id.clone(),
        session_type: record.session_type.clone().unwrap_or_default(),
        message_type: record.message_type.clone().unwrap_or_default(),
        persisted_at: Some(system_time_to_timestamp(record.persisted_at)),
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
