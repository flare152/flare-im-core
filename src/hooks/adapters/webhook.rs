use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::error::{ErrorBuilder, ErrorCode, Result};

use super::super::config::HookDefinition;
use super::super::types::{
    DeliveryEvent, DeliveryHook, HookContext, HookOutcome, MessageDraft, MessageRecord,
    PostSendHook, PreSendDecision, PreSendHook, RecallEvent, RecallHook,
};

#[derive(Clone)]
pub struct WebhookHookFactory {
    client: Client,
}

impl WebhookHookFactory {
    pub fn new() -> Result<Self> {
        let client = Client::builder().use_rustls_tls().build().map_err(|err| {
            ErrorBuilder::new(ErrorCode::ConfigurationError, "failed to build http client")
                .details(err.to_string())
                .build_error()
        })?;
        Ok(Self { client })
    }

    pub fn build_pre_send(
        &self,
        def: &HookDefinition,
        endpoint: &str,
        secret: Option<String>,
        headers: HashMap<String, String>,
    ) -> Arc<dyn PreSendHook> {
        Arc::new(WebhookPreSendHook {
            client: self.client.clone(),
            endpoint: endpoint.to_string(),
            secret,
            headers,
            static_metadata: def.metadata.clone(),
        })
    }

    pub fn build_post_send(
        &self,
        def: &HookDefinition,
        endpoint: &str,
        secret: Option<String>,
        headers: HashMap<String, String>,
    ) -> Arc<dyn PostSendHook> {
        Arc::new(WebhookPostSendHook {
            client: self.client.clone(),
            endpoint: endpoint.to_string(),
            secret,
            headers,
            static_metadata: def.metadata.clone(),
        })
    }

    pub fn build_delivery(
        &self,
        def: &HookDefinition,
        endpoint: &str,
        secret: Option<String>,
        headers: HashMap<String, String>,
    ) -> Arc<dyn DeliveryHook> {
        Arc::new(WebhookDeliveryHook {
            client: self.client.clone(),
            endpoint: endpoint.to_string(),
            secret,
            headers,
            static_metadata: def.metadata.clone(),
        })
    }

    pub fn build_recall(
        &self,
        def: &HookDefinition,
        endpoint: &str,
        secret: Option<String>,
        headers: HashMap<String, String>,
    ) -> Arc<dyn RecallHook> {
        Arc::new(WebhookRecallHook {
            client: self.client.clone(),
            endpoint: endpoint.to_string(),
            secret,
            headers,
            static_metadata: def.metadata.clone(),
        })
    }
}

#[derive(Serialize)]
struct WebhookContextPayload<'a> {
    tenant_id: &'a str,
    conversation_id: Option<&'a str>,
    conversation_type: Option<&'a str>,
    message_type: Option<&'a str>,
    sender_id: Option<&'a str>,
    trace_id: Option<&'a str>,
    tags: &'a HashMap<String, String>,
    attributes: &'a HashMap<String, String>,
}

#[derive(Serialize, Deserialize, Clone)]
struct WebhookDraftPayload {
    message_id: Option<String>,
    client_message_id: Option<String>,
    conversation_id: Option<String>,
    payload: String,
    headers: HashMap<String, String>,
    metadata: HashMap<String, String>,
}

impl From<&MessageDraft> for WebhookDraftPayload {
    fn from(value: &MessageDraft) -> Self {
        Self {
            message_id: value.message_id.clone(),
            client_message_id: value.client_message_id.clone(),
            conversation_id: value.conversation_id.clone(),
            payload: STANDARD.encode(&value.payload),
            headers: value.headers.clone(),
            metadata: value.metadata.clone(),
        }
    }
}

impl WebhookDraftPayload {
    fn apply_to(self, draft: &mut MessageDraft) -> Result<()> {
        if let Some(id) = self.message_id {
            draft.message_id = Some(id);
        }
        if let Some(id) = self.client_message_id {
            draft.client_message_id = Some(id);
        }
        if let Some(conv) = self.conversation_id {
            draft.conversation_id = Some(conv);
        }
        draft.payload = STANDARD.decode(self.payload).map_err(|err| {
            ErrorBuilder::new(
                ErrorCode::DeserializationError,
                "invalid payload encoding from webhook",
            )
            .details(err.to_string())
            .build_error()
        })?;
        draft.headers = self.headers;
        draft.metadata = self.metadata;
        Ok(())
    }
}

#[derive(Serialize)]
struct PreSendWebhookRequest<'a> {
    context: WebhookContextPayload<'a>,
    draft: WebhookDraftPayload,
    metadata: &'a HashMap<String, String>,
}

#[derive(Deserialize)]
struct PreSendWebhookResponse {
    allow: bool,
    draft: Option<WebhookDraftPayload>,
    #[serde(default)]
    status: Option<WebhookStatus>,
}

#[derive(Deserialize)]
struct WebhookStatus {
    code: Option<String>,
    message: Option<String>,
}

fn build_headers(
    request_builder: reqwest::RequestBuilder,
    secret: &Option<String>,
    headers: &HashMap<String, String>,
) -> reqwest::RequestBuilder {
    let mut builder = request_builder;
    builder = builder.header("content-type", "application/json");
    if let Some(secret) = secret {
        builder = builder.header("x-flare-signature", secret);
    }
    for (key, value) in headers {
        builder = builder.header(key, value);
    }
    builder
}

fn webhook_context<'a>(ctx: &'a HookContext) -> WebhookContextPayload<'a> {
    WebhookContextPayload {
        tenant_id: &ctx.tenant_id,
        conversation_id: ctx.conversation_id.as_deref(),
        conversation_type: ctx.conversation_type.as_deref(),
        message_type: ctx.message_type.as_deref(),
        sender_id: ctx.sender_id.as_deref(),
        trace_id: ctx.trace_id.as_deref(),
        tags: &ctx.tags,
        attributes: &ctx.attributes,
    }
}

#[derive(Clone)]
struct WebhookPreSendHook {
    client: Client,
    endpoint: String,
    secret: Option<String>,
    headers: HashMap<String, String>,
    static_metadata: HashMap<String, String>,
}

#[async_trait]
impl PreSendHook for WebhookPreSendHook {
    async fn handle(&self, ctx: &HookContext, draft: &mut MessageDraft) -> PreSendDecision {
        let request_body = PreSendWebhookRequest {
            context: webhook_context(ctx),
            draft: WebhookDraftPayload::from(&*draft),
            metadata: &self.static_metadata,
        };

        let builder = self.client.post(&self.endpoint);
        let builder = build_headers(builder, &self.secret, &self.headers);
        let response = builder.json(&request_body).send().await;

        match response {
            Ok(resp) => match resp.json::<PreSendWebhookResponse>().await {
                Ok(payload) => {
                    if payload.allow {
                        if let Some(draft_payload) = payload.draft {
                            if let Err(err) = draft_payload.apply_to(draft) {
                                return PreSendDecision::Reject { error: err };
                            }
                        }
                        PreSendDecision::Continue
                    } else {
                        let err = payload
                            .status
                            .and_then(|status| {
                                let code = status.code.unwrap_or_else(|| "BusinessRejected".into());
                                let message = status
                                    .message
                                    .unwrap_or_else(|| "rejected by webhook".into());
                                Some(
                                    ErrorBuilder::new(ErrorCode::OperationFailed, &message)
                                        .details(code)
                                        .build_error(),
                                )
                            })
                            .unwrap_or_else(|| {
                                ErrorBuilder::new(
                                    ErrorCode::OperationFailed,
                                    "webhook rejected message",
                                )
                                .build_error()
                            });
                        PreSendDecision::Reject { error: err }
                    }
                }
                Err(err) => PreSendDecision::Reject {
                    error: ErrorBuilder::new(
                        ErrorCode::DeserializationError,
                        "failed to decode webhook response",
                    )
                    .details(err.to_string())
                    .build_error(),
                },
            },
            Err(err) => PreSendDecision::Reject {
                error: ErrorBuilder::new(ErrorCode::ServiceUnavailable, "webhook request failed")
                    .details(err.to_string())
                    .build_error(),
            },
        }
    }
}

#[derive(Serialize)]
struct PostSendWebhookRequest<'a> {
    context: WebhookContextPayload<'a>,
    record: &'a MessageRecord,
    draft: WebhookDraftPayload,
    metadata: &'a HashMap<String, String>,
}

#[derive(Clone)]
struct WebhookPostSendHook {
    client: Client,
    endpoint: String,
    secret: Option<String>,
    headers: HashMap<String, String>,
    static_metadata: HashMap<String, String>,
}

#[async_trait]
impl PostSendHook for WebhookPostSendHook {
    async fn handle(
        &self,
        ctx: &HookContext,
        record: &MessageRecord,
        draft: &MessageDraft,
    ) -> HookOutcome {
        let request_body = PostSendWebhookRequest {
            context: webhook_context(ctx),
            record,
            draft: WebhookDraftPayload::from(draft),
            metadata: &self.static_metadata,
        };

        let builder = self.client.post(&self.endpoint);
        let builder = build_headers(builder, &self.secret, &self.headers);
        match builder.json(&request_body).send().await {
            Ok(resp) if resp.status().is_success() => HookOutcome::Completed,
            Ok(resp) => {
                let err =
                    ErrorBuilder::new(ErrorCode::ServiceUnavailable, "webhook post-send failed")
                        .details(resp.status().to_string())
                        .build_error();
                HookOutcome::Failed(err)
            }
            Err(err) => {
                let err =
                    ErrorBuilder::new(ErrorCode::ServiceUnavailable, "webhook post-send failed")
                        .details(err.to_string())
                        .build_error();
                HookOutcome::Failed(err)
            }
        }
    }
}

#[derive(Serialize)]
struct DeliveryWebhookRequest<'a> {
    context: WebhookContextPayload<'a>,
    event: &'a DeliveryEvent,
    metadata: &'a HashMap<String, String>,
}

#[derive(Clone)]
struct WebhookDeliveryHook {
    client: Client,
    endpoint: String,
    secret: Option<String>,
    headers: HashMap<String, String>,
    static_metadata: HashMap<String, String>,
}

#[async_trait]
impl DeliveryHook for WebhookDeliveryHook {
    async fn handle(&self, ctx: &HookContext, event: &DeliveryEvent) -> HookOutcome {
        let request_body = DeliveryWebhookRequest {
            context: webhook_context(ctx),
            event,
            metadata: &self.static_metadata,
        };
        let builder = self.client.post(&self.endpoint);
        let builder = build_headers(builder, &self.secret, &self.headers);
        match builder.json(&request_body).send().await {
            Ok(resp) if resp.status().is_success() => HookOutcome::Completed,
            Ok(resp) => {
                let err =
                    ErrorBuilder::new(ErrorCode::ServiceUnavailable, "webhook delivery failed")
                        .details(resp.status().to_string())
                        .build_error();
                HookOutcome::Failed(err)
            }
            Err(err) => {
                let err =
                    ErrorBuilder::new(ErrorCode::ServiceUnavailable, "webhook delivery failed")
                        .details(err.to_string())
                        .build_error();
                HookOutcome::Failed(err)
            }
        }
    }
}

#[derive(Serialize)]
struct RecallWebhookRequest<'a> {
    context: WebhookContextPayload<'a>,
    event: &'a RecallEvent,
    metadata: &'a HashMap<String, String>,
}

#[derive(Clone)]
struct WebhookRecallHook {
    client: Client,
    endpoint: String,
    secret: Option<String>,
    headers: HashMap<String, String>,
    static_metadata: HashMap<String, String>,
}

#[async_trait]
impl RecallHook for WebhookRecallHook {
    async fn handle(&self, ctx: &HookContext, event: &RecallEvent) -> HookOutcome {
        let request_body = RecallWebhookRequest {
            context: webhook_context(ctx),
            event,
            metadata: &self.static_metadata,
        };
        let builder = self.client.post(&self.endpoint);
        let builder = build_headers(builder, &self.secret, &self.headers);

        match builder.json(&request_body).send().await {
            Ok(resp) if resp.status().is_success() => HookOutcome::Completed,
            Ok(resp) => {
                let err = ErrorBuilder::new(ErrorCode::ServiceUnavailable, "webhook recall failed")
                    .details(resp.status().to_string())
                    .build_error();
                HookOutcome::Failed(err)
            }
            Err(err) => {
                let err = ErrorBuilder::new(ErrorCode::ServiceUnavailable, "webhook recall failed")
                    .details(err.to_string())
                    .build_error();
                HookOutcome::Failed(err)
            }
        }
    }
}
