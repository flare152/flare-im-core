//! 推送领域服务 - 包含所有业务逻辑实现

use std::sync::Arc;

use anyhow::{Context as AnyhowContext, Result};
use flare_im_core::hooks::{MessageDraft, MessageRecord};
use flare_im_core::hooks::hook_context_data::{HookContextData, set_hook_context_data};
use flare_server_core::context::{Context, ContextExt};
use flare_proto::flare::push::v1::{PushAckRequest, PushAckResponse};
use flare_proto::push::{
    PushFailure, PushMessageRequest, PushMessageResponse, PushNotificationRequest,
    PushNotificationResponse,
};
use tracing::{info, instrument, warn};
use uuid::Uuid;

use crate::domain::repositories::PushEventPublisher;
use crate::infrastructure::validator::RequestValidator;
use flare_im_core::hooks::HookDispatcher;

/// 推送领域服务 - 包含所有业务逻辑
pub struct PushDomainService {
    publisher: Arc<dyn PushEventPublisher>,
    validator: Arc<dyn RequestValidator>,
    hook_dispatcher: HookDispatcher,
}

impl PushDomainService {
    pub fn new(
        publisher: Arc<dyn PushEventPublisher>,
        validator: Arc<dyn RequestValidator>,
        hook_dispatcher: HookDispatcher,
    ) -> Self {
        Self {
            publisher,
            validator,
            hook_dispatcher,
        }
    }

    /// 入队推送消息（业务逻辑）
    #[instrument(skip(self, ctx), fields(
        request_id = %ctx.request_id(),
        trace_id = %ctx.trace_id(),
        user_count = request.user_ids.len()
    ))]
    pub async fn enqueue_message(
        &self,
        ctx: &Context,
        request: PushMessageRequest,
    ) -> Result<PushMessageResponse> {
        ctx.ensure_not_cancelled()?;
        
        // 1. 入参校验
        self.validator
            .validate_message_request(&request)
            .with_context(|| "Request validation failed")?;

        let user_ids = request.user_ids.clone();
        let task_id = Uuid::new_v4().to_string();

        // 2. 发布到 Kafka（幂等性由 Kafka 保证）
        match self.publisher.publish_message(&request).await {
            Ok(_) => {
                // 3. PostSend Hook（异步，不阻塞响应）
                // 注意：PostSend Hook 在 proxy 中只做审计日志，不修改消息状态
                // 实际的消息状态由 server/worker 处理
                tokio::spawn({
                    let hook_dispatcher = self.hook_dispatcher.clone();
                    let request = request.clone();
                    let task_id = task_id.clone();
                    let user_count = user_ids.len();
                    async move {
                        // 调用 PostSend Hook（审计日志）
                        tracing::info!(
                            task_id = %task_id,
                            user_count = user_count,
                            "Push message enqueued successfully"
                        );

                        // 构造 Hook 上下文和消息记录
                        let tenant_id = request
                            .tenant
                            .as_ref()
                            .map(|t| t.tenant_id.clone())
                            .unwrap_or_else(|| "default".to_string());

                        // 创建 Context
                        let mut ctx = Context::with_request_id(task_id.clone());
                        ctx = ctx.with_trace_id(task_id.clone());
                        if !tenant_id.is_empty() {
                            ctx = ctx.with_tenant_id(tenant_id);
                        }
                        
                        // 创建 HookContextData
                        let conversation_id = request.message.as_ref().map(|m| m.conversation_id.clone());
                        let conversation_type = request.message.as_ref().map(|m| {
                            match m.conversation_type {
                                1 => "single".to_string(),
                                2 => "group".to_string(),
                                3 => "broadcast".to_string(),
                                _ => "unknown".to_string(),
                            }
                        });
                        let message_type = request.message.as_ref().map(|m| {
                            match m.message_type {
                                1 => "text".to_string(),
                                2 => "image".to_string(),
                                3 => "audio".to_string(),
                                4 => "video".to_string(),
                                5 => "file".to_string(),
                                6 => "location".to_string(),
                                7 => "contact".to_string(),
                                8 => "system".to_string(),
                                9 => "custom".to_string(),
                                _ => "unknown".to_string(),
                            }
                        });
                        let sender_id = request.message.as_ref().map(|m| m.sender_id.clone());
                        
                        if let Some(conv_id) = &conversation_id {
                            ctx = ctx.with_session_id(conv_id.clone());
                        }
                        
                        let hook_data = HookContextData::new()
                            .with_conversation_id(conversation_id.unwrap_or_default())
                            .with_conversation_type(conversation_type.unwrap_or_default())
                            .with_message_type(message_type.unwrap_or_default())
                            .with_sender_id(sender_id.unwrap_or_default())
                            .occurred_now();
                        
                        ctx = set_hook_context_data(ctx, hook_data);

                        let payload = Vec::new();
                        let mut draft = MessageDraft::new(payload);

                        // 从请求选项中获取元数据
                        if let Some(options) = &request.options {
                            draft.metadata = options.metadata.clone();
                        }

                        let record = MessageRecord {
                            message_id: request
                                .message
                                .as_ref()
                                .map(|m| m.server_id.clone())
                                .unwrap_or_default(),
                            client_message_id: request
                                .message
                                .as_ref()
                                .map(|m| m.client_msg_id.clone()),
                            conversation_id: request
                                .message
                                .as_ref()
                                .map(|m| m.conversation_id.clone())
                                .unwrap_or_default(),
                            sender_id: request
                                .message
                                .as_ref()
                                .map(|m| m.sender_id.clone())
                                .unwrap_or_default(),
                            conversation_type: request
                                .message
                                .as_ref()
                                .map(|m| {
                                    let conversation_type_str = match m.conversation_type {
                                        1 => "single".to_string(),
                                        2 => "group".to_string(),
                                        3 => "broadcast".to_string(),
                                        _ => "unknown".to_string(),
                                    };
                                    Some(conversation_type_str)
                                })
                                .flatten(),
                            message_type: request
                                .message
                                .as_ref()
                                .map(|m| {
                                    let message_type_str = match m.message_type {
                                        1 => "text".to_string(),
                                        2 => "image".to_string(),
                                        3 => "audio".to_string(),
                                        4 => "video".to_string(),
                                        5 => "file".to_string(),
                                        6 => "location".to_string(),
                                        7 => "contact".to_string(),
                                        8 => "system".to_string(),
                                        9 => "custom".to_string(),
                                        _ => "unknown".to_string(),
                                    };
                                    Some(message_type_str)
                                })
                                .flatten(),
                            persisted_at: std::time::SystemTime::now(),
                            metadata: if let Some(options) = &request.options {
                                options.metadata.clone()
                            } else {
                                std::collections::HashMap::new()
                            },
                        };

                        // 执行 PostSend Hook
                        if let Err(e) = hook_dispatcher.post_send(&ctx, &record, &draft).await {
                            tracing::warn!(
                                task_id = %task_id,
                                error = %e,
                                "Failed to execute PostSend hook"
                            );
                        } else {
                            tracing::debug!(
                                task_id = %task_id,
                                "Successfully executed PostSend hook"
                            );
                        }
                    }
                });

                Ok(PushMessageResponse {
                    success_count: user_ids.len() as i32,
                    fail_count: 0,
                    failed_user_ids: Vec::new(),
                    failures: Vec::new(),
                    task_id,
                    status: Some(rpc_status_success()),
                })
            }
            Err(err) => {
                warn!(
                    error = %err,
                    task_id = %task_id,
                    "Failed to publish message to Kafka"
                );

                let failures: Vec<PushFailure> = user_ids
                    .iter()
                    .map(|user_id| PushFailure {
                        user_id: user_id.clone(),
                        code: error_code_internal(),
                        error_message: err.to_string(),
                        metadata: std::collections::HashMap::new(),
                    })
                    .collect();

                Ok(PushMessageResponse {
                    success_count: 0,
                    fail_count: user_ids.len() as i32,
                    failed_user_ids: user_ids.clone(),
                    failures,
                    task_id: String::new(),
                    status: Some(rpc_status_internal("failed to enqueue push message")),
                })
            }
        }
    }

    /// 入队推送通知（业务逻辑）
    #[instrument(skip(self, ctx), fields(
        request_id = %ctx.request_id(),
        trace_id = %ctx.trace_id(),
        user_count = request.user_ids.len()
    ))]
    pub async fn enqueue_notification(
        &self,
        ctx: &Context,
        request: PushNotificationRequest,
    ) -> Result<PushNotificationResponse> {
        ctx.ensure_not_cancelled()?;
        // 1. 入参校验
        self.validator
            .validate_notification_request(&request)
            .with_context(|| "Request validation failed")?;

        let user_ids = request.user_ids.clone();
        let task_id = Uuid::new_v4().to_string();

        // 2. 发布到 Kafka（幂等性由 Kafka 保证）
        match self.publisher.publish_notification(&request).await {
            Ok(_) => {
                // 3. PostSend Hook（异步，不阻塞响应）
                tokio::spawn({
                    let hook_dispatcher = self.hook_dispatcher.clone();
                    let request = request.clone();
                    let task_id = task_id.clone();
                    let user_count = user_ids.len();
                    async move {
                        // 调用 PostSend Hook（审计日志）
                        tracing::info!(
                            task_id = %task_id,
                            user_count = user_count,
                            "Push notification enqueued successfully"
                        );

                        // 构造 Hook 上下文和消息记录
                        let tenant_id = request
                            .tenant
                            .as_ref()
                            .map(|t| t.tenant_id.clone())
                            .unwrap_or_else(|| "default".to_string());

                        // 创建 Context
                        let mut ctx = Context::with_request_id(task_id.clone());
                        ctx = ctx.with_trace_id(task_id.clone());
                        if !tenant_id.is_empty() {
                            ctx = ctx.with_tenant_id(tenant_id);
                        }
                        
                        // 创建 HookContextData
                        let hook_data = HookContextData::new()
                            .with_message_type("notification".to_string())
                            .occurred_now();
                        
                        ctx = set_hook_context_data(ctx, hook_data);

                        let content = if let Some(notification) = &request.notification {
                            notification.title.clone() + ": " + &notification.body
                        } else {
                            "Notification".to_string()
                        };
                        let payload = content.as_bytes().to_vec();
                        let mut draft = MessageDraft::new(payload);

                        // 从请求选项中获取元数据
                        if let Some(options) = &request.options {
                            draft.metadata = options.metadata.clone();
                        }

                        let record = MessageRecord {
                            message_id: Uuid::new_v4().to_string(),
                            client_message_id: None,
                            conversation_id: "push_notification".to_string(),
                            sender_id: "system".to_string(), // 系统发送的通知
                            conversation_type: None,
                            message_type: Some("notification".to_string()),
                            persisted_at: std::time::SystemTime::now(),
                            metadata: if let Some(options) = &request.options {
                                options.metadata.clone()
                            } else {
                                std::collections::HashMap::new()
                            },
                        };

                        // 执行 PostSend Hook
                        if let Err(e) = hook_dispatcher.post_send(&ctx, &record, &draft).await {
                            tracing::warn!(
                                task_id = %task_id,
                                error = %e,
                                "Failed to execute PostSend hook"
                            );
                        } else {
                            tracing::debug!(
                                task_id = %task_id,
                                "Successfully executed PostSend hook"
                            );
                        }
                    }
                });

                Ok(PushNotificationResponse {
                    success_count: user_ids.len() as i32,
                    fail_count: 0,
                    failures: Vec::new(),
                    task_id,
                    status: Some(rpc_status_success()),
                })
            }
            Err(err) => {
                warn!(
                    error = %err,
                    task_id = %task_id,
                    "Failed to publish notification to Kafka"
                );

                let failures: Vec<PushFailure> = user_ids
                    .iter()
                    .map(|user_id| PushFailure {
                        user_id: user_id.clone(),
                        code: error_code_internal(),
                        error_message: err.to_string(),
                        metadata: std::collections::HashMap::new(),
                    })
                    .collect();

                Ok(PushNotificationResponse {
                    success_count: 0,
                    fail_count: user_ids.len() as i32,
                    failures,
                    task_id: String::new(),
                    status: Some(rpc_status_internal("failed to enqueue push notification")),
                })
            }
        }
    }

    /// 入队 ACK（业务逻辑）
    #[instrument(skip(self, ctx), fields(
        request_id = %ctx.request_id(),
        trace_id = %ctx.trace_id(),
        message_id = %request.ack.as_ref().map(|a| a.server_msg_id.as_str()).unwrap_or("")
    ))]
    pub async fn enqueue_ack(&self, ctx: &Context, request: PushAckRequest) -> Result<PushAckResponse> {
        ctx.ensure_not_cancelled()?;
        
        // 1. 入参校验
        if request.ack.is_none() {
            return Err(anyhow::anyhow!("ack is required"));
        }

        if request.target_user_ids.is_empty() {
            return Err(anyhow::anyhow!("target_user_ids cannot be empty"));
        }

        let user_ids = request.target_user_ids.clone();
        let message_id = request
            .ack
            .as_ref()
            .map(|a| a.server_msg_id.clone())
            .unwrap_or_default();

        // 2. 发布到 Kafka（幂等性由 Kafka 保证）
        match self.publisher.publish_ack(&request).await {
            Ok(_) => {
                info!(
                    message_id = %message_id,
                    user_count = user_ids.len(),
                    "ACK enqueued successfully"
                );

                Ok(PushAckResponse {
                    success_count: user_ids.len() as i32,
                    fail_count: 0,
                    failed_user_ids: Vec::new(),
                    status: Some(rpc_status_success()),
                })
            }
            Err(err) => {
                warn!(
                    error = %err,
                    message_id = %message_id,
                    "Failed to publish ACK to Kafka"
                );

                Ok(PushAckResponse {
                    success_count: 0,
                    fail_count: user_ids.len() as i32,
                    failed_user_ids: user_ids.clone(),
                    status: Some(rpc_status_internal("failed to enqueue push ACK")),
                })
            }
        }
    }
}

fn error_code_internal() -> i32 {
    flare_proto::common::ErrorCode::Internal as i32
}

fn rpc_status_success() -> flare_proto::common::RpcStatus {
    flare_proto::common::RpcStatus {
        code: flare_proto::common::ErrorCode::Ok as i32,
        message: String::new(),
        details: Default::default(),
        context: None,
    }
}

fn rpc_status_internal(message: &str) -> flare_proto::common::RpcStatus {
    flare_proto::common::RpcStatus {
        code: error_code_internal(),
        message: message.to_string(),
        details: Default::default(),
        context: None,
    }
}
