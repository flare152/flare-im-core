//! 消息领域服务 - 包含所有业务逻辑实现

use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use flare_im_core::hooks::HookDispatcher;
use flare_im_core::tracing::create_span;
use flare_proto::push::{PushMessageRequest, PushOptions};
use flare_proto::storage::StoreMessageRequest;
use prost::Message;
use tracing::{Span, instrument};

use crate::domain::model::MessageProfile;
use crate::domain::model::{MessageDefaults, MessageSubmission};
use crate::domain::repository::{
    MessageEventPublisher, MessageEventPublisherItem, SessionRepository, SessionRepositoryItem,
    WalRepository, WalRepositoryItem,
};
use crate::domain::service::hook_builder::{
    apply_draft_to_request, build_draft_from_request, build_hook_context, build_message_record,
    draft_from_submission, merge_context,
};
use crate::domain::service::sequence_allocator::SequenceAllocator;

/// 消息领域服务 - 包含所有业务逻辑
pub struct MessageDomainService {
    publisher: Arc<MessageEventPublisherItem>,
    wal_repository: Arc<WalRepositoryItem>,
    session_repository: Option<Arc<SessionRepositoryItem>>,
    /// 序列号分配器（核心能力：保证同会话消息顺序）
    sequence_allocator: Arc<SequenceAllocator>,
    defaults: MessageDefaults,
    hooks: Arc<HookDispatcher>,
}

impl MessageDomainService {
    pub fn new(
        publisher: Arc<MessageEventPublisherItem>,
        wal_repository: Arc<WalRepositoryItem>,
        session_repository: Option<Arc<SessionRepositoryItem>>,
        sequence_allocator: Arc<SequenceAllocator>,
        defaults: MessageDefaults,
        hooks: Arc<HookDispatcher>,
    ) -> Self {
        Self {
            publisher,
            wal_repository,
            session_repository,
            sequence_allocator,
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
        let _start = Instant::now(); // 添加下划线前缀表示故意未使用
        let _span = Span::current(); // 添加下划线前缀表示故意未使用

        // 先提取租户ID（在借用request之前）
        let tenant_id = request
            .tenant
            .as_ref()
            .map(|t| t.tenant_id.as_str())
            .or_else(|| self.defaults.default_tenant_id.as_deref())
            .unwrap_or("unknown")
            .to_string();

        // 设置追踪属性
        {
            // 由于缺少相应的追踪函数，暂时注释掉这些调用
            // set_tenant_id(&_span, &tenant_id);
            if let Some(message) = &request.message {
                if !message.id.is_empty() {
                    // set_message_id(&_span, &message.id);
                    _span.record("message_id", &message.id);
                }
                if !message.sender_id.is_empty() {
                    // set_user_id(&_span, &message.sender_id);
                }
                _span.record("message_type", message.message_type);
            }
        }

        let original_context =
            build_hook_context(&request, self.defaults.default_tenant_id.as_ref());
        let mut draft =
            build_draft_from_request(&request).context("Failed to build draft from request")?;

        // 执行 PreSend Hook（如果启用）
        if execute_pre_send {
            let _hook_span = create_span("message-orchestrator", "pre_send_hook");

            self.hooks
                .pre_send(&original_context, &mut draft)
                .await
                .context("PreSend hook failed")?;

            // 让 _hook_span 离开作用域以结束 span

            apply_draft_to_request(&mut request, &draft);
        }

        let updated_context =
            build_hook_context(&request, self.defaults.default_tenant_id.as_ref());
        let hook_context = merge_context(&original_context, updated_context);

        let submission = MessageSubmission::prepare(request, &self.defaults)
            .context("Failed to prepare message")?;

        // 🔹 核心能力：分配 session_seq（保证消息顺序）
        // 参考微信 MsgService 设计：每个会话维护独立的递增序列号
        let session_seq = match self
            .sequence_allocator
            .allocate_seq(&submission.message.session_id, &tenant_id)
            .await
        {
            Ok(seq) => {
                tracing::debug!(
                    session_id = %submission.message.session_id,
                    seq = seq,
                    "Allocated session sequence"
                );
                seq
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    session_id = %submission.message.session_id,
                    "Redis unavailable for sequence allocation, using degraded mode"
                );
                // 降级策略：使用时间戳 + 随机数（不保证严格顺序，但保证趋势递增）
                self.sequence_allocator.allocate_seq_degraded()
            }
        };

        // 注入 seq 到消息中（将在 Kafka 发布时使用）
        // 注意：这里需要修改 MessageSubmission 中的 message
        // 由于 submission 是不可变的，我们需要在 build_push_request 中处理
        // 或者修改 MessageSubmission 为可变

        // 临时方案：将 seq 存储在 extra 字段中
        let mut submission = submission;
        submission.message.seq = session_seq;

        // 获取消息类型信息（用于判断是否需要持久化）
        // 注意：MessageProfile::ensure 会修改 message，所以需要 clone
        let mut message_for_profile = submission.message.clone();
        let profile = MessageProfile::ensure(&mut message_for_profile);
        let processing_type = profile.processing_type(); // 保留变量名，因为在后面会使用

        let _message_type = match processing_type {
            // 添加下划线前缀表示故意未使用
            crate::domain::model::message_kind::MessageProcessingType::Normal => "normal",
            crate::domain::model::message_kind::MessageProcessingType::Notification => {
                "notification"
            }
        };

        // 仅普通消息需要写入WAL
        if profile.needs_wal() {
            let _wal_span = create_span("message-orchestrator", "wal_write");

            self.wal_repository
                .append(&submission)
                .await
                .context("Failed to append WAL entry")?;

            // 让 _wal_span 离开作用域以结束 span
        }

        // 确保 session 存在（异步化，不阻塞消息发送流程）
        if let Some(session_repo) = &self.session_repository {
            // 提取 participants（发送者 + 接收者）
            let mut participants = vec![submission.message.sender_id.clone()];

            // 单聊：添加接收者
            if submission.message.session_type == flare_proto::common::SessionType::Single as i32 {
                if !submission.message.receiver_id.is_empty() {
                    participants.push(submission.message.receiver_id.clone());
                }
            }
            // 群聊/频道：participants 只包含发送者，成员列表由推送服务查询

            // 提取参数（克隆用于异步任务）
            let session_id = submission.kafka_payload.session_id.clone();
            let session_type =
                match flare_proto::common::SessionType::try_from(submission.message.session_type) {
                    Ok(st) => match st {
                        flare_proto::common::SessionType::Single => "single".to_string(),
                        flare_proto::common::SessionType::Group => "group".to_string(),
                        flare_proto::common::SessionType::Channel => "channel".to_string(),
                        _ => "unknown".to_string(),
                    },
                    Err(_) => "unknown".to_string(),
                };
            let business_type = submission.message.business_type.clone();
            let tenant_id = submission
                .kafka_payload
                .tenant
                .as_ref()
                .map(|t| t.tenant_id.clone());
            let session_repo_clone = session_repo.clone();

            // 异步创建 Session，不阻塞主流程
            tokio::spawn(async move {
                if let Err(e) = session_repo_clone
                    .ensure_session(
                        &session_id,
                        &session_type,
                        &business_type,
                        participants,
                        tenant_id.as_deref(),
                    )
                    .await
                {
                    tracing::warn!(
                        "Failed to ensure session asynchronously: {}, session_id={}",
                        e,
                        session_id
                    );
                } else {
                    tracing::debug!("Session ensured asynchronously: session_id={}", session_id);
                }
            });
        }

        // 构建推送任务
        let push_request = self.build_push_request(&submission, &profile)?;

        // 根据消息类型决定发布策略
        let _kafka_span = create_span("message-orchestrator", "kafka_produce");

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

        // 让 _kafka_span 离开作用域以结束 span

        let record = build_message_record(&submission, &submission.kafka_payload);
        let post_draft =
            draft_from_submission(&submission).context("Failed to build draft from submission")?;

        // 执行 PostSend Hook（系统消息也需要执行，用于通知业务系统）
        self.hooks
            .post_send(&hook_context, &record, &post_draft)
            .await
            .context("PostSend hook failed")?;

        Ok(submission.message_id)
    }

    /// 构建推送请求
    ///
    /// 优化：优先使用 receiver_id 和 channel_id，避免查询会话服务
    fn build_push_request(
        &self,
        submission: &MessageSubmission,
        profile: &MessageProfile,
    ) -> Result<PushMessageRequest> {
        // 提取接收者ID列表（优先使用 receiver_id 和 channel_id）
        let mut user_ids = Vec::new();

        if let Ok(session_type) =
            flare_proto::common::SessionType::try_from(submission.message.session_type)
        {
            match session_type {
                flare_proto::common::SessionType::Single => {
                    // 单聊：优先使用 receiver_id，性能最优
                    if !submission.message.receiver_id.is_empty() {
                        user_ids.push(submission.message.receiver_id.clone());
                        tracing::debug!(
                            "Single chat message using receiver_id: session_id={}, sender_id={}, receiver_id={}",
                            submission.message.session_id,
                            submission.message.sender_id,
                            submission.message.receiver_id
                        );
                    } else {
                        // receiver_id 为空，降级到从 session_id 提取（向后兼容）
                        tracing::warn!(
                            "Single chat message missing receiver_id, falling back to session_id extraction. session_id={}, sender_id={}",
                            submission.message.session_id,
                            submission.message.sender_id
                        );
                        if let Some(participants) = self.extract_participants_from_session_id(
                            &submission.message.session_id,
                            &submission.message.sender_id,
                        ) {
                            user_ids = participants;
                        }
                    }
                }
                flare_proto::common::SessionType::Group
                | flare_proto::common::SessionType::Channel => {
                    // 群聊、频道：使用 channel_id 或 session_id 查询成员
                    // user_ids 留空，由推送服务根据 channel_id/session_id 查询成员
                    let channel_id = if !submission.message.channel_id.is_empty() {
                        &submission.message.channel_id
                    } else {
                        &submission.message.session_id
                    };
                    tracing::debug!(
                        "Group/channel message. Push worker will query members. channel_id={}, session_id={}",
                        channel_id,
                        submission.message.session_id
                    );
                }
                _ => {}
            }
        }

        // 克隆消息并清理字段，确保所有字符串字段都是有效的 UTF-8
        // 这是为了避免 Protobuf 解码错误
        let mut message_for_push = submission.message.clone();

        // 验证 receiver_id 和 channel_id 在克隆后仍然存在
        if message_for_push.session_type == 1 {
            if message_for_push.receiver_id.is_empty() {
                tracing::error!(
                    message_id = %message_for_push.id,
                    session_id = %message_for_push.session_id,
                    sender_id = %message_for_push.sender_id,
                    "Single chat message missing receiver_id after clone"
                );
                anyhow::bail!(
                    "Single chat message must provide receiver_id. message_id={}, session_id={}, sender_id={}",
                    message_for_push.id,
                    message_for_push.session_id,
                    message_for_push.sender_id
                );
            }
        }

        // 清理字符串字段，确保它们是有效的 UTF-8 字符串
        // 注意：新版 Message 结构已移除 sender_platform_id、sender_nickname、sender_avatar_url、group_id 等字段
        // 这些信息现在通过 attributes 或 extra 字段存储
        // 但 receiver_id 和 channel_id 仍然是 Message 的字段，必须保留
        message_for_push.client_msg_id =
            String::from_utf8_lossy(message_for_push.client_msg_id.as_bytes()).to_string();
        message_for_push.recall_reason =
            String::from_utf8_lossy(message_for_push.recall_reason.as_bytes()).to_string();

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
            require_online: profile.processing_type()
                == crate::domain::model::message_kind::MessageProcessingType::Notification,
            persist_if_offline: profile.processing_type()
                == crate::domain::model::message_kind::MessageProcessingType::Normal,
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

    /// 从会话ID中提取参与者
    ///
    /// 注意：新格式（1-{hash}）无法从哈希反推用户ID，需要查询会话服务获取参与者
    ///
    /// # 参数
    /// * `session_id` - 会话ID（格式：1-{hash}）
    /// * `sender_id` - 发送者ID（用于过滤）
    ///
    /// # 返回
    /// * `None` - 新格式无法直接解析，需要查询会话服务
    fn extract_participants_from_session_id(
        &self,
        _session_id: &str,
        _sender_id: &str,
    ) -> Option<Vec<String>> {
        // 新格式（1-{hash}）无法从哈希反推用户ID，返回None
        // 调用方需要查询会话服务获取参与者
        None
    }
}
