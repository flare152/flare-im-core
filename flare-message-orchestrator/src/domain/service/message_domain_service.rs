//! 消息领域服务 - 包含所有业务逻辑实现

use std::sync::Arc;
use std::time::Instant;

use anyhow::{Result, Context};
use flare_im_core::hooks::HookDispatcher;
#[cfg(feature = "tracing")]
use flare_im_core::tracing::{create_span, set_message_id, set_user_id, set_tenant_id, set_error};
use flare_proto::storage::StoreMessageRequest;
use flare_proto::push::{PushMessageRequest, PushOptions};
use prost::Message;
use tracing::{instrument, Span};

use crate::domain::service::hook_builder::{
    apply_draft_to_request, build_draft_from_request, build_hook_context, build_message_record,
    draft_from_submission, merge_context,
};
use crate::domain::model::MessageProfile;
use crate::domain::model::{MessageDefaults, MessageSubmission};
use crate::domain::repository::{MessageEventPublisher, MessageEventPublisherItem, SessionRepository, SessionRepositoryItem, WalRepository, WalRepositoryItem};

/// 消息领域服务 - 包含所有业务逻辑
pub struct MessageDomainService {
    publisher: Arc<MessageEventPublisherItem>,
    wal_repository: Arc<WalRepositoryItem>,
    session_repository: Option<Arc<SessionRepositoryItem>>,
    defaults: MessageDefaults,
    hooks: Arc<HookDispatcher>,
}

impl MessageDomainService {
    pub fn new(
        publisher: Arc<MessageEventPublisherItem>,
        wal_repository: Arc<WalRepositoryItem>,
        session_repository: Option<Arc<SessionRepositoryItem>>,
        defaults: MessageDefaults,
        hooks: Arc<HookDispatcher>,
    ) -> Self {
        Self {
            publisher,
            wal_repository,
            session_repository,
            defaults,
            hooks,
        }
    }

    /// 编排消息存储流程（业务逻辑）
    /// 按照"PreSend Hook → WAL → Kafka → PostSend Hook"的顺序编排消息写入流程
    #[instrument(skip(self), fields(tenant_id, message_id, message_type))]
    pub async fn orchestrate_message_storage(
        &self,
        mut request: StoreMessageRequest,
        execute_pre_send: bool,
    ) -> Result<String> {
        let start = Instant::now();
        let span = Span::current();
        
        // 先提取租户ID（在借用request之前）
        let tenant_id = request.tenant.as_ref()
            .map(|t| t.tenant_id.as_str())
            .or_else(|| self.defaults.default_tenant_id.as_deref())
            .unwrap_or("unknown")
            .to_string();
        
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
        let mut draft = build_draft_from_request(&request)
            .context("Failed to build draft from request")?;

        // 执行 PreSend Hook（如果启用）
        if execute_pre_send {
            #[cfg(feature = "tracing")]
            let hook_span = create_span("message-orchestrator", "pre_send_hook");
            
            self.hooks.pre_send(&original_context, &mut draft).await
                .context("PreSend hook failed")?;
            
            #[cfg(feature = "tracing")]
            hook_span.end();
            
            apply_draft_to_request(&mut request, &draft);
        }

        let updated_context =
            build_hook_context(&request, self.defaults.default_tenant_id.as_ref());
        let hook_context = merge_context(&original_context, updated_context);

        let submission = MessageSubmission::prepare(request, &self.defaults)
            .context("Failed to prepare message")?;

        // 获取消息类型信息（用于判断是否需要持久化）
        // 注意：MessageProfile::ensure 会修改 message，所以需要 clone
        let mut message_for_profile = submission.message.clone();
        let profile = MessageProfile::ensure(&mut message_for_profile);
        let processing_type = profile.processing_type();
        
        let message_type = match processing_type {
            crate::domain::model::message_kind::MessageProcessingType::Normal => "normal",
            crate::domain::model::message_kind::MessageProcessingType::Notification => "notification",
        };

        // 仅普通消息需要写入WAL
        if profile.needs_wal() {
            #[cfg(feature = "tracing")]
            let wal_span = create_span("message-orchestrator", "wal_write");
            
            self.wal_repository.append(&submission).await
                .context("Failed to append WAL entry")?;
            
            #[cfg(feature = "tracing")]
            wal_span.end();
        }

        // 确保 session 存在（异步化，不阻塞消息发送流程）
        if let Some(session_repo) = &self.session_repository {
            // 提取 participants（发送者 + 接收者）
            let mut participants = vec![submission.message.sender_id.clone()];
            participants.extend(submission.message.receiver_ids.clone());
            if !submission.message.receiver_id.is_empty() 
                && !participants.contains(&submission.message.receiver_id) {
                participants.push(submission.message.receiver_id.clone());
            }
            
            // 提取参数（克隆用于异步任务）
            let session_id = submission.kafka_payload.session_id.clone();
            let session_type = submission.message.session_type.clone();
            let business_type = submission.message.business_type.clone();
            let tenant_id = submission.kafka_payload.tenant.as_ref()
                .map(|t| t.tenant_id.clone());
            let session_repo_clone = session_repo.clone();
            
            // 异步创建 Session，不阻塞主流程
            tokio::spawn(async move {
                if let Err(e) = session_repo_clone.ensure_session(
                    &session_id,
                    &session_type,
                    &business_type,
                    participants,
                    tenant_id.as_deref(),
                ).await {
                    tracing::warn!(
                        error = %e,
                        session_id = %session_id,
                        "Failed to ensure session asynchronously, continuing with message publish"
                    );
                } else {
                    tracing::debug!(
                        session_id = %session_id,
                        "Session ensured asynchronously"
                    );
                }
            });
        }

        // 构建推送任务
        let push_request = self.build_push_request(&submission, &profile)?;

        // 根据消息类型决定发布策略
        #[cfg(feature = "tracing")]
        let kafka_span = create_span("message-orchestrator", "kafka_produce");
        
        match processing_type {
            crate::domain::model::message_kind::MessageProcessingType::Normal => {
                // 普通消息：并行发布到存储队列和推送队列
                self.publisher
                    .publish_both(submission.kafka_payload.clone(), push_request)
                    .await
                    .context("Failed to publish message event")?;
            }
            crate::domain::model::message_kind::MessageProcessingType::Notification => {
                // 通知消息：仅发布到推送队列
                self.publisher
                    .publish_push(push_request)
                    .await
                    .context("Failed to publish push task")?;
            }
        }
        
        #[cfg(feature = "tracing")]
        kafka_span.end();

        let record = build_message_record(&submission, &submission.kafka_payload);
        let post_draft = draft_from_submission(&submission)
            .context("Failed to build draft from submission")?;

        // 执行 PostSend Hook（系统消息也需要执行，用于通知业务系统）
        self.hooks
            .post_send(&hook_context, &record, &post_draft)
            .await
            .context("PostSend hook failed")?;

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
        
        // 如果没有receiver_ids，使用receiver_id（单聊场景）
        if user_ids.is_empty() && !submission.message.receiver_id.is_empty() {
            user_ids.push(submission.message.receiver_id.clone());
        }
        
        // 如果是聊天室消息且 user_ids 为空，则查询所有在线用户
        // 注意：对于聊天室消息，我们查询该会话的所有在线用户，而不是要求业务系统提供所有成员列表
        if user_ids.is_empty() && (submission.message.session_type == "group" || submission.message.business_type == "chatroom") {
            if !submission.message.session_id.is_empty() {
                // 尝试通过 Session 服务查询会话的所有参与者，然后查询在线用户
                // 注意：这里我们暂时只记录警告，因为消息编排器没有直接访问在线服务的接口
                // 实际上，这个逻辑应该在 push-worker 或 push-proxy 中处理
                tracing::warn!(
                    session_id = %submission.message.session_id,
                    session_type = %submission.message.session_type,
                    business_type = %submission.message.business_type,
                    "Group/chatroom message has empty receiver_ids. Push worker should query online users for this session."
                );
            } else {
                tracing::warn!(
                    session_id = %submission.message.session_id,
                    session_type = %submission.message.session_type,
                    business_type = %submission.message.business_type,
                    "Group/chatroom message has empty receiver_ids and session_id. Cannot query online users."
                );
            }
        }

        // 克隆消息并清理字段，确保所有字符串字段都是有效的 UTF-8
        // 这是为了避免 Protobuf 解码错误（特别是 sender_platform_id 字段）
        let mut message_for_push = submission.message.clone();
        
        // 清理 sender_platform_id，确保它是有效的 UTF-8 字符串
        // 如果字段包含非 UTF-8 数据，将其设置为空字符串
        message_for_push.sender_platform_id = String::from_utf8_lossy(
            message_for_push.sender_platform_id.as_bytes()
        ).to_string();
        
        // 清理其他字符串字段（以防万一）
        message_for_push.sender_nickname = String::from_utf8_lossy(
            message_for_push.sender_nickname.as_bytes()
        ).to_string();
        message_for_push.sender_avatar_url = String::from_utf8_lossy(
            message_for_push.sender_avatar_url.as_bytes()
        ).to_string();
        message_for_push.group_id = String::from_utf8_lossy(
            message_for_push.group_id.as_bytes()
        ).to_string();
        message_for_push.client_msg_id = String::from_utf8_lossy(
            message_for_push.client_msg_id.as_bytes()
        ).to_string();
        message_for_push.recall_reason = String::from_utf8_lossy(
            message_for_push.recall_reason.as_bytes()
        ).to_string();
        
        // 验证消息大小，防止异常大的消息
        // 先序列化消息以计算大小
        let message_bytes = message_for_push.encode_to_vec();
        const MAX_MESSAGE_SIZE: usize = 10 * 1024 * 1024; // 10MB
        if message_bytes.len() > MAX_MESSAGE_SIZE {
            anyhow::bail!(
                "Message size {} bytes exceeds maximum allowed size {} bytes",
                message_bytes.len(),
                MAX_MESSAGE_SIZE
            );
        }

        // 构建推送选项
        let push_options = PushOptions {
            require_online: profile.processing_type() == crate::domain::model::message_kind::MessageProcessingType::Notification,
            persist_if_offline: profile.processing_type() == crate::domain::model::message_kind::MessageProcessingType::Normal,
            priority: 5, // 默认优先级
            metadata: std::collections::HashMap::new(),
            channel: String::new(),
            mute_when_quiet: false,
        };

        Ok(PushMessageRequest {
            user_ids,
            message: Some(message_for_push),
            options: Some(push_options),
            context: submission.kafka_payload.context.clone(),
            tenant: submission.kafka_payload.tenant.clone(),
            template_id: String::new(),
            template_data: std::collections::HashMap::new(),
        })
    }
}

