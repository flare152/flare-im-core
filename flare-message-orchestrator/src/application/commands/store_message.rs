use std::sync::Arc;
use std::time::Instant;

use flare_im_core::error::{ErrorBuilder, ErrorCode, Result};
use flare_im_core::hooks::HookDispatcher;
use flare_im_core::metrics::MessageOrchestratorMetrics;
#[cfg(feature = "tracing")]
use flare_im_core::tracing::{create_span, set_message_id, set_user_id, set_tenant_id, set_error};
use flare_proto::storage::StoreMessageRequest;
use flare_proto::push::{PushMessageRequest, PushOptions};
use prost::Message;
use tracing::{instrument, Span};

use crate::application::hooks::{
    apply_draft_to_request, build_draft_from_request, build_hook_context, build_message_record,
    draft_from_submission, merge_context,
};
use crate::domain::message_kind::MessageProfile;
use crate::domain::message_submission::{MessageDefaults, MessageSubmission};
use crate::domain::repositories::{MessageEventPublisher, WalRepository};

/// 命令对象，封装单条存储请求。
#[derive(Debug)]
pub struct StoreMessageCommand {
    pub request: StoreMessageRequest,
}

/// 消息编排层 StoreMessage 用例处理器，负责执行业务 Hook、WAL 及 Kafka 投递。
pub struct StoreMessageCommandHandler {
    publisher: Arc<dyn MessageEventPublisher + Send + Sync>,
    wal_repository: Arc<dyn WalRepository + Send + Sync>,
    defaults: MessageDefaults,
    hooks: Arc<HookDispatcher>,
    metrics: Arc<MessageOrchestratorMetrics>,
}

impl StoreMessageCommandHandler {
    pub fn new(
        publisher: Arc<dyn MessageEventPublisher + Send + Sync>,
        wal_repository: Arc<dyn WalRepository + Send + Sync>,
        defaults: MessageDefaults,
        hooks: Arc<HookDispatcher>,
        metrics: Arc<MessageOrchestratorMetrics>,
    ) -> Self {
        Self {
            publisher,
            wal_repository,
            defaults,
            hooks,
            metrics,
        }
    }

    /// 按照"PreSend Hook → WAL → Kafka → PostSend Hook"的顺序编排消息写入流程。
    pub async fn handle(&self, command: StoreMessageCommand) -> Result<String> {
        self.handle_with_hooks(command, true).await
    }

    /// 处理消息存储，支持跳过 PreSend Hook（用于系统消息）
    #[instrument(skip(self), fields(tenant_id, message_id, message_type))]
    pub async fn handle_with_hooks(
        &self,
        command: StoreMessageCommand,
        execute_pre_send: bool,
    ) -> Result<String> {
        let start = Instant::now();
        let span = Span::current();
        
        // 先提取租户ID（在借用request之前）
        let tenant_id = command.request.tenant.as_ref()
            .map(|t| t.tenant_id.as_str())
            .or_else(|| self.defaults.default_tenant_id.as_deref())
            .unwrap_or("unknown")
            .to_string();
        
        let mut request = command.request;

        // 设置追踪属性
        #[cfg(feature = "tracing")]
        {
            set_tenant_id(&span, &tenant_id);
            if let Some(message) = &request.message {
                if !message.message_id.is_empty() {
                    set_message_id(&span, &message.message_id);
                    span.record("message_id", &message.message_id);
                }
                if !message.sender_id.is_empty() {
                    set_user_id(&span, &message.sender_id);
                }
                span.record("message_type", message.message_type);
            }
        }

        let original_context =
            build_hook_context(&request, self.defaults.default_tenant_id.as_ref());
        let mut draft = build_draft_from_request(&request);

        // 执行 PreSend Hook（如果启用）
        if execute_pre_send {
            #[cfg(feature = "tracing")]
            let hook_span = create_span("message-orchestrator", "pre_send_hook");
            
            let hook_start = Instant::now();
            match self.hooks.pre_send(&original_context, &mut draft).await {
                Ok(_) => {
                    let hook_duration = hook_start.elapsed();
                    self.metrics.pre_send_hook_duration_seconds.observe(hook_duration.as_secs_f64());
                    #[cfg(feature = "tracing")]
                    hook_span.end();
                }
                Err(e) => {
                    let hook_duration = hook_start.elapsed();
                    self.metrics.pre_send_hook_duration_seconds.observe(hook_duration.as_secs_f64());
                    // 记录Hook失败（如果有hook名称）
                    self.metrics.pre_send_hook_failure_total
                        .with_label_values(&["unknown", &tenant_id])
                        .inc();
                    #[cfg(feature = "tracing")]
                    {
                        set_error(&hook_span, &e.to_string());
                        hook_span.end();
                    }
                    return Err(e);
                }
            }
            apply_draft_to_request(&mut request, &draft);
        }

        let updated_context =
            build_hook_context(&request, self.defaults.default_tenant_id.as_ref());
        let hook_context = merge_context(&original_context, updated_context);

        let submission = MessageSubmission::prepare(request, &self.defaults).map_err(|err| {
            ErrorBuilder::new(ErrorCode::InvalidParameter, "failed to prepare message")
                .details(err.to_string())
                .build_error()
        })?;

        // 获取消息类型信息（用于判断是否需要持久化）
        // 注意：MessageProfile::ensure 会修改 message，所以需要 clone
        let mut message_for_profile = submission.message.clone();
        let profile = MessageProfile::ensure(&mut message_for_profile);
        let processing_type = profile.processing_type();
        
        let message_type = match processing_type {
            crate::domain::message_kind::MessageProcessingType::Normal => "normal",
            crate::domain::message_kind::MessageProcessingType::Notification => "notification",
        };

        // 仅普通消息需要写入WAL
        if profile.needs_wal() {
            #[cfg(feature = "tracing")]
            let wal_span = create_span("message-orchestrator", "wal_write");
            
            let wal_start = Instant::now();
            match self.wal_repository.append(&submission).await {
                Ok(_) => {
                    let wal_duration = wal_start.elapsed();
                    self.metrics.wal_write_duration_seconds.observe(wal_duration.as_secs_f64());
                    #[cfg(feature = "tracing")]
                    wal_span.end();
                }
                Err(err) => {
                    let wal_duration = wal_start.elapsed();
                    self.metrics.wal_write_duration_seconds.observe(wal_duration.as_secs_f64());
                    self.metrics.wal_write_failure_total.inc();
                    #[cfg(feature = "tracing")]
                    {
                        set_error(&wal_span, &err.to_string());
                        wal_span.end();
                    }
                    return Err(ErrorBuilder::new(ErrorCode::InternalError, "failed to append wal entry")
                        .details(err.to_string())
                        .build_error());
                }
            }
        }

        // 构建推送任务
        let push_request = self.build_push_request(&submission, &profile)?;

        // 根据消息类型决定发布策略
        #[cfg(feature = "tracing")]
        let kafka_span = create_span("message-orchestrator", "kafka_produce");
        
        let kafka_start = Instant::now();
        let kafka_result = match processing_type {
            crate::domain::message_kind::MessageProcessingType::Normal => {
                // 普通消息：并行发布到存储队列和推送队列
                self.publisher
                    .publish_both(submission.kafka_payload.clone(), push_request)
                    .await
                    .map_err(|err| {
                        ErrorBuilder::new(
                            ErrorCode::ServiceUnavailable,
                            "failed to publish message event",
                        )
                        .details(err.to_string())
                        .build_error()
                    })
            }
            crate::domain::message_kind::MessageProcessingType::Notification => {
                // 通知消息：仅发布到推送队列
                self.publisher
                    .publish_push(push_request)
                    .await
                    .map_err(|err| {
                        ErrorBuilder::new(
                            ErrorCode::ServiceUnavailable,
                            "failed to publish push task",
                        )
                        .details(err.to_string())
                        .build_error()
                    })
            }
        };
        
        let kafka_duration = kafka_start.elapsed();
        self.metrics.kafka_produce_duration_seconds.observe(kafka_duration.as_secs_f64());
        
        match &kafka_result {
            Ok(_) => {
                // 成功，不记录失败
                #[cfg(feature = "tracing")]
                kafka_span.end();
            }
            Err(e) => {
                // 记录Kafka生产失败
                let topic = match processing_type {
                    crate::domain::message_kind::MessageProcessingType::Normal => "both",
                    crate::domain::message_kind::MessageProcessingType::Notification => "push",
                };
                self.metrics.kafka_produce_failure_total
                    .with_label_values(&[topic, &tenant_id])
                    .inc();
                #[cfg(feature = "tracing")]
                {
                    set_error(&kafka_span, &e.to_string());
                    kafka_span.end();
                }
            }
        }
        
        kafka_result?;

        let record = build_message_record(&submission, &submission.kafka_payload);
        let post_draft = draft_from_submission(&submission);

        // 执行 PostSend Hook（系统消息也需要执行，用于通知业务系统）
        self.hooks
            .post_send(&hook_context, &record, &post_draft)
            .await?;

        // 记录总耗时和总数
        let total_duration = start.elapsed();
        self.metrics.messages_sent_duration_seconds.observe(total_duration.as_secs_f64());
        self.metrics.messages_sent_total
            .with_label_values(&[message_type, &tenant_id])
            .inc();

        Ok(submission.message_id)
    }

    /// 构建推送请求
    fn build_push_request(
        &self,
        submission: &MessageSubmission,
        profile: &MessageProfile,
    ) -> Result<PushMessageRequest> {
        // 提取接收者ID列表
        let mut user_ids = submission.message.receiver_ids.clone();
        
        // 如果没有receiver_ids，使用receiver_id
        if user_ids.is_empty() && !submission.message.receiver_id.is_empty() {
            user_ids.push(submission.message.receiver_id.clone());
        }
        
        // 注意：如果 user_ids 仍然为空（聊天室消息），Push Server 会识别并查询所有在线用户
        // 这是预期的行为，不需要在这里处理

        // 序列化消息为bytes
        let message_bytes = submission.message.encode_to_vec();
        
        // 验证消息大小，防止异常大的消息
        const MAX_MESSAGE_SIZE: usize = 10 * 1024 * 1024; // 10MB
        if message_bytes.len() > MAX_MESSAGE_SIZE {
            tracing::error!(
                message_id = %submission.message.id,
                session_id = %submission.message.session_id,
                message_size = message_bytes.len(),
                max_size = MAX_MESSAGE_SIZE,
                "Message size exceeds maximum allowed size when building push request"
            );
            return Err(ErrorBuilder::new(
                ErrorCode::InvalidParameter,
                "message size exceeds maximum allowed size",
            )
            .details(format!(
                "Message size {} bytes exceeds maximum allowed size {} bytes",
                message_bytes.len(),
                MAX_MESSAGE_SIZE
            ))
            .build_error());
        }

        // 构建推送选项
        let push_options = PushOptions {
            require_online: profile.processing_type() == crate::domain::message_kind::MessageProcessingType::Notification,
            persist_if_offline: profile.processing_type() == crate::domain::message_kind::MessageProcessingType::Normal,
            priority: 5, // 默认优先级
            metadata: std::collections::HashMap::new(),
            channel: String::new(),
            mute_when_quiet: false,
        };

        Ok(PushMessageRequest {
            user_ids,
            message: message_bytes,
            options: Some(push_options),
            context: submission.kafka_payload.context.clone(),
            tenant: submission.kafka_payload.tenant.clone(),
            template_id: String::new(),
            template_data: std::collections::HashMap::new(),
        })
    }
}
