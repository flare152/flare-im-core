//! 命令处理器（编排层）- 轻量级，只负责编排领域服务和记录指标

use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use flare_im_core::metrics::MessageOrchestratorMetrics;
use flare_server_core::context::{Context, ContextExt};
use flare_im_core::utils::context::require_context;
use tracing::instrument;

use crate::application::commands::{
    AddReactionCommand, BatchMarkMessageReadCommand, BatchSendMessageCommand,
    BatchStoreMessageCommand, DeleteMessageCommand, EditMessageCommand,
    HandleTemporaryMessageCommand, MarkAllConversationsReadCommand,
    MarkConversationReadCommand, MarkMessageCommand, PinMessageCommand,
    ReadMessageCommand, RecallMessageCommand, RemoveReactionCommand, SendMessageCommand,
    StoreMessageCommand, UnmarkMessageCommand, UnpinMessageCommand,
};
use crate::domain::service::MessageDomainService;
use crate::domain::service::message_operation_service::MessageOperationService;
use crate::domain::service::message_temporary_service::MessageTemporaryService;

/// 消息命令处理器（编排层）
pub struct MessageCommandHandler {
    domain_service: Arc<MessageDomainService>,
    operation_service: Arc<MessageOperationService>,
    temporary_service: Option<Arc<MessageTemporaryService>>,
    metrics: Arc<MessageOrchestratorMetrics>,
}

impl MessageCommandHandler {
    pub fn new(
        domain_service: Arc<MessageDomainService>,
        operation_service: Arc<MessageOperationService>,
        temporary_service: Option<Arc<MessageTemporaryService>>,
        metrics: Arc<MessageOrchestratorMetrics>,
    ) -> Self {
        Self {
            domain_service,
            operation_service,
            temporary_service,
            metrics,
        }
    }

    /// 处理存储消息命令
    #[instrument(skip(self, ctx), fields(
        request_id = %ctx.request_id(),
        trace_id = %ctx.trace_id(),
        tenant_id = %ctx.tenant_id().unwrap_or("0"),
    ))]
    pub async fn handle_store_message(&self, ctx: &Context, command: StoreMessageCommand) -> Result<(String, u64)> {
        ctx.ensure_not_cancelled()?;
        let start = Instant::now();

        let tenant_id = ctx.tenant_id().unwrap_or("0").to_string();

        let message_type = command
            .request
            .message
            .as_ref()
            .map(|m| match m.message_type {
                0 => "normal",
                _ => "notification",
            })
            .unwrap_or("normal")
            .to_string();

        let tenant_id = ctx.tenant_id().unwrap_or("0").to_string();
        
        let result = self
            .domain_service
            .orchestrate_message_storage(ctx, command.request, true)
            .await;

        // 记录指标
        let duration = start.elapsed();
        self.metrics
            .messages_sent_duration_seconds
            .observe(duration.as_secs_f64());

        if result.is_ok() {
            self.metrics
                .messages_sent_total
                .with_label_values(&[message_type, tenant_id])
                .inc();
        }

        result
    }

    /// 处理存储消息命令（跳过 PreSend Hook）
    #[instrument(skip(self))]
    pub async fn handle_store_message_without_pre_hook(
        &self,
        command: StoreMessageCommand,
    ) -> Result<(String, u64)> {
        let start = Instant::now();

        // 提取租户ID和消息类型用于指标标签（在移动之前）
        let tenant_id = command
            .request
            .tenant
            .as_ref()
            .map(|t| t.tenant_id.as_str())
            .unwrap_or("unknown")
            .to_string();

        let message_type = command
            .request
            .message
            .as_ref()
            .map(|m| match m.message_type {
                0 => "normal",
                _ => "notification",
            })
            .unwrap_or("normal")
            .to_string();

        // 从 command.request 构建 Context
        let ctx = if let Some(tenant) = &command.request.tenant {
            Context::root().with_tenant_id(tenant.tenant_id.clone())
        } else {
            Context::root()
        };
        let result = self
            .domain_service
            .orchestrate_message_storage(&ctx, command.request, false)
            .await;

        // 记录指标
        let duration = start.elapsed();
        self.metrics
            .messages_sent_duration_seconds
            .observe(duration.as_secs_f64());

        if result.is_ok() {
            self.metrics
                .messages_sent_total
                .with_label_values(&[&message_type, &tenant_id])
                .inc();
        }

        result
    }

    /// 处理批量存储消息命令
    #[instrument(skip(self), fields(batch_size = command.requests.len()))]
    pub async fn handle_batch_store_message(
        &self,
        ctx: &Context,
        command: BatchStoreMessageCommand,
    ) -> Result<Vec<String>> {
        let mut message_ids = Vec::new();
        for request in command.requests {
            // 从 request 构建 Context
            let request_ctx = if let Some(tenant) = &request.tenant {
                Context::root().with_tenant_id(tenant.tenant_id.clone())
            } else {
                ctx.clone()
            };
            match self
                .domain_service
                .orchestrate_message_storage(&request_ctx, request, true)
                .await
            {
                Ok((message_id, _seq)) => message_ids.push(message_id),
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to store message in batch");
                    // 继续处理其他消息
                }
            }
        }
        Ok(message_ids)
    }

    /// 处理撤回消息命令
    #[instrument(skip(self), fields(message_id = %cmd.base.message_id))]
    pub async fn handle_recall_message(&self, cmd: RecallMessageCommand) -> Result<()> {
        self.operation_service.handle_recall(cmd).await
    }

    /// 处理编辑消息命令
    #[instrument(skip(self), fields(message_id = %cmd.base.message_id))]
    pub async fn handle_edit_message(&self, cmd: EditMessageCommand) -> Result<()> {
        self.operation_service.handle_edit(cmd).await
    }

    /// 处理删除消息命令
    #[instrument(skip(self), fields(message_id = %cmd.base.message_id))]
    pub async fn handle_delete_message(&self, cmd: DeleteMessageCommand) -> Result<()> {
        self.operation_service.handle_delete(cmd).await
    }

    /// 处理标记已读命令
    #[instrument(skip(self), fields(message_id = %cmd.base.message_id))]
    pub async fn handle_read_message(&self, cmd: ReadMessageCommand) -> Result<()> {
        self.operation_service.handle_read(cmd).await
    }

    /// 处理添加反应命令
    #[instrument(skip(self), fields(message_id = %cmd.base.message_id, emoji = %cmd.emoji))]
    pub async fn handle_add_reaction(&self, cmd: AddReactionCommand) -> Result<i32> {
        self.operation_service.handle_add_reaction(cmd).await
    }

    /// 处理移除反应命令
    #[instrument(skip(self), fields(message_id = %cmd.base.message_id, emoji = %cmd.emoji))]
    pub async fn handle_remove_reaction(&self, cmd: RemoveReactionCommand) -> Result<i32> {
        self.operation_service.handle_remove_reaction(cmd).await
    }

    /// 处理置顶消息命令
    #[instrument(skip(self), fields(message_id = %cmd.base.message_id))]
    pub async fn handle_pin_message(&self, cmd: PinMessageCommand) -> Result<()> {
        self.operation_service.handle_pin(cmd).await
    }

    /// 处理取消置顶消息命令
    #[instrument(skip(self), fields(message_id = %cmd.base.message_id))]
    pub async fn handle_unpin_message(&self, cmd: UnpinMessageCommand) -> Result<()> {
        self.operation_service.handle_unpin(cmd).await
    }

    /// 处理标记消息命令
    #[instrument(skip(self), fields(message_id = %cmd.base.message_id))]
    pub async fn handle_mark_message(&self, cmd: MarkMessageCommand) -> Result<()> {
        self.operation_service.handle_mark(cmd).await
    }

    /// 处理取消标记消息命令
    #[instrument(skip(self), fields(message_id = %cmd.base.message_id))]
    pub async fn handle_unmark_message(&self, cmd: UnmarkMessageCommand) -> Result<()> {
        self.operation_service.handle_unmark(cmd).await
    }

    /// 处理临时消息命令（只推送，不持久化）
    #[instrument(skip(self, ctx), fields(
        request_id = %ctx.request_id(),
        trace_id = %ctx.trace_id(),
        message_id = %cmd.message.server_id
    ))]
    pub async fn handle_temporary_message(&self, ctx: &Context, cmd: HandleTemporaryMessageCommand) -> Result<()> {
        ctx.ensure_not_cancelled()?;
        
        let temporary_service = self
            .temporary_service
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Temporary message service not configured"))?;

        temporary_service
            .handle_temporary_message(ctx, &cmd.message)
            .await
    }

    /// 处理发送消息命令（包含消息类别判断和路由）
    ///
    /// 根据消息类别路由到不同处理流程：
    /// - Temporary: 临时消息（只推送，不持久化）
    /// - Operation: 操作消息（通过 NotificationContent 传递）
    /// - Normal/Notification: 普通消息（存储编排）
    #[instrument(skip(self, ctx), fields(
        request_id = %ctx.request_id(),
        trace_id = %ctx.trace_id(),
        message_id = %cmd.message.server_id,
        conversation_id = %cmd.conversation_id
    ))]
    pub async fn handle_send_message(
        &self,
        ctx: &Context,
        mut cmd: SendMessageCommand,
    ) -> Result<(String, u64)> {
        ctx.ensure_not_cancelled()?;
        
        // 如果 cmd.tenant 为空，从 ctx 中提取 tenant_id 并设置
        if cmd.tenant.is_none() || cmd.tenant.as_ref().map(|t| t.tenant_id.is_empty()).unwrap_or(true) {
            if let Some(tenant_id) = ctx.tenant_id() {
                if !tenant_id.is_empty() {
                    cmd.tenant = Some(flare_proto::common::TenantContext {
                        tenant_id: tenant_id.to_string(),
                        business_type: String::new(),
                        environment: String::new(),
                        organization_id: String::new(),
                        labels: std::collections::HashMap::new(),
                        attributes: std::collections::HashMap::new(),
                    });
                }
            }
        }
        
        use crate::domain::model::message_kind::MessageProfile;

        let mut message = cmd.message.clone();

        // 判断消息类别（使用 MessageProfile）
        let profile = MessageProfile::ensure(&mut message);
        let category = profile.category();

        tracing::info!(
            message_id = %message.server_id,
            message_type = message.message_type,
            category = ?category,
            conversation_id = %message.conversation_id,
            sender_id = %message.sender_id,
            "处理发送消息，判断消息类别"
        );

        // 根据消息类别路由到不同处理流程
        match category {
            crate::domain::model::message_kind::MessageCategory::Temporary => {
                // 临时消息：只推送，不持久化
                let temp_cmd = HandleTemporaryMessageCommand {
                    message: message.clone(),
                };
                self.handle_temporary_message(ctx, temp_cmd).await?;

                // 临时消息返回消息ID和seq=0
                Ok((message.server_id, 0))
            }
            crate::domain::model::message_kind::MessageCategory::Operation => {
                // 操作消息：直接提取 MessageOperation 并执行操作
                if let Some(flare_proto::common::message_content::Content::Operation(operation)) =
                    message.content.as_ref().and_then(|c| c.content.as_ref())
                {
                    // 执行操作
                    self.execute_operation(ctx, operation, &message, &cmd).await?;

                    // 操作消息返回目标消息ID和seq=0（操作不产生新消息）
                    // 但操作结果会通过推送消息通知用户
                    Ok((operation.target_message_id.clone(), 0))
                } else {
                    // 无法提取操作，降级为普通消息
                    tracing::warn!(
                        message_id = %message.server_id,
                        "Operation message without MessageOperation, fallback to normal message"
                    );
                    self.handle_normal_message(ctx, cmd).await
                }
            }
            _ => {
                // Notification 和 Normal 消息：普通消息处理（存储编排）
                self.handle_normal_message(ctx, cmd).await
            }
        }
    }

    /// 处理普通消息（内部方法）
    async fn handle_normal_message(&self, ctx: &Context, cmd: SendMessageCommand) -> Result<(String, u64)> {
        ctx.ensure_not_cancelled()?;
        // 验证单聊消息必须包含 receiver_id
        if cmd.message.conversation_type == flare_proto::common::ConversationType::Single as i32 {
            if cmd.message.receiver_id.is_empty() {
                return Err(anyhow::anyhow!(
                    "Single chat message must provide receiver_id. message_id={}, conversation_id={}, sender_id={}",
                    cmd.message.server_id, cmd.message.conversation_id, cmd.message.sender_id
                ));
            }
        }

        // 从 Context 中提取 RequestContext 和 TenantContext
        let context = ctx.request().cloned().map(|rc| rc.into());
        
        // 优先从 Context 中提取 tenant，如果 Context 中没有，则使用 cmd.tenant
        let tenant = ctx.tenant().cloned()
            .map(|tc| tc.into())
            .or_else(|| {
                // 如果 Context 中没有完整的 TenantContext，但 ctx.tenant_id() 有值，则构建 TenantContext
                ctx.tenant_id()
                    .filter(|id| !id.is_empty())
                    .map(|tenant_id| {
                        flare_proto::common::TenantContext {
                            tenant_id: tenant_id.to_string(),
                            business_type: String::new(),
                            environment: String::new(),
                            organization_id: String::new(),
                            labels: std::collections::HashMap::new(),
                            attributes: std::collections::HashMap::new(),
                        }
                    })
            })
            .or(cmd.tenant.clone());
        
        // 将 SendMessageCommand 转换为 StoreMessageRequest
        let store_request = flare_proto::storage::StoreMessageRequest {
            conversation_id: cmd.conversation_id.clone(),
            message: Some(cmd.message),
            sync: cmd.sync,
            // 从 Context 中获取 context 和 tenant
            context,
            tenant,
            tags: std::collections::HashMap::new(),
        };

        // 调用存储消息命令处理
        self.handle_store_message(ctx, StoreMessageCommand {
            request: store_request,
        })
        .await
    }

    /// 处理批量发送消息命令
    ///
    /// 返回成功和失败的结果
    #[instrument(skip(self, ctx), fields(
        request_id = %ctx.request_id(),
        trace_id = %ctx.trace_id(),
        batch_size = cmd.requests.len()
    ))]
    pub async fn handle_batch_send_message(
        &self,
        ctx: &Context,
        cmd: BatchSendMessageCommand,
    ) -> Result<(Vec<(String, u64)>, Vec<String>)> {
        ctx.ensure_not_cancelled()?;
        let mut successes = Vec::new();
        let mut failures = Vec::new();

        for send_req in cmd.requests {
            // 优先使用请求中的 tenant，如果没有则使用 Context 中的
            let tenant = send_req.tenant.clone();

            let message = match send_req.message {
                Some(msg) => msg,
                None => {
                    failures.push("message is required".to_string());
                    continue;
                }
            };

            let send_cmd = SendMessageCommand {
                message,
                conversation_id: send_req.conversation_id.clone(),
                sync: send_req.sync,
                context: send_req.context,
                tenant,
            };

            match self.handle_send_message(ctx, send_cmd).await {
                Ok((message_id, seq)) => {
                    successes.push((message_id, seq));
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to send message in batch");
                    failures.push(e.to_string());
                }
            }
        }

        Ok((successes, failures))
    }

    /// 执行消息操作
    ///
    /// 注意：operation 参数直接是 MessageOperation，不需要从 NotificationContent 中提取
    async fn execute_operation(
        &self,
        ctx: &Context,
        operation: &flare_proto::common::MessageOperation,
        message: &flare_proto::common::Message,
        cmd: &SendMessageCommand,
    ) -> Result<()> {
        ctx.ensure_not_cancelled()?;
        use flare_proto::common::{OperationType, message_operation::OperationData};
        use crate::application::commands::{
            RecallMessageCommand, EditMessageCommand, DeleteMessageCommand,
            ReadMessageCommand, AddReactionCommand, RemoveReactionCommand,
            PinMessageCommand, UnpinMessageCommand, MarkMessageCommand, UnmarkMessageCommand,
            MessageOperationCommand,
        };

        let tenant_id = ctx.tenant_id().unwrap_or("0").to_string();

        let base_cmd = MessageOperationCommand {
            message_id: operation.target_message_id.clone(),
            operator_id: operation.operator_id.clone(),
            timestamp: operation.timestamp.as_ref()
                .map(|ts| chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32))
                .flatten()
                .unwrap_or_else(|| chrono::Utc::now()),
            tenant_id: tenant_id.to_string(),
            conversation_id: message.conversation_id.clone(),
        };

        match OperationType::try_from(operation.operation_type) {
            Ok(OperationType::Recall) => {
                let recall_data = match &operation.operation_data {
                    Some(OperationData::Recall(data)) => data,
                    _ => return Err(anyhow::anyhow!("Recall operation requires RecallOperationData")),
                };

                let recall_cmd = RecallMessageCommand {
                    base: base_cmd,
                    reason: if recall_data.reason.is_empty() {
                        None
                    } else {
                        Some(recall_data.reason.clone())
                    },
                    time_limit_seconds: if recall_data.time_limit_seconds == 0 {
                        None
                    } else {
                        Some(recall_data.time_limit_seconds)
                    },
                };

                self.handle_recall_message(recall_cmd).await
            }
            Ok(OperationType::Edit) => {
                let edit_data = match &operation.operation_data {
                    Some(OperationData::Edit(data)) => data,
                    _ => return Err(anyhow::anyhow!("Edit operation requires EditOperationData")),
                };

                let new_content_bytes = &edit_data.new_content;

                let edit_cmd = EditMessageCommand {
                    base: base_cmd,
                    new_content: new_content_bytes.clone(),
                    reason: if edit_data.reason.is_empty() {
                        None
                    } else {
                        Some(edit_data.reason.clone())
                    },
                };

                self.handle_edit_message(edit_cmd).await
            }
            Ok(OperationType::Delete) => {
                let delete_data = match &operation.operation_data {
                    Some(OperationData::Delete(data)) => data,
                    _ => return Err(anyhow::anyhow!("Delete operation requires DeleteOperationData")),
                };

                // 从 metadata 中获取要删除的消息ID列表
                let message_ids: Vec<String> = operation
                    .metadata
                    .get("message_ids")
                    .map(|s| s.split(',').map(|s| s.to_string()).collect())
                    .unwrap_or_else(|| vec![operation.target_message_id.clone()]);

                let delete_cmd = DeleteMessageCommand {
                    base: base_cmd,
                    delete_type: crate::application::commands::DeleteType::Soft, // 默认软删除
                    reason: if delete_data.reason.is_empty() {
                        None
                    } else {
                        Some(delete_data.reason.clone())
                    },
                    target_user_id: None, // 全局删除时为空
                    message_ids,
                    notify_others: delete_data.notify_others,
                };

                self.handle_delete_message(delete_cmd).await
            }
            Ok(OperationType::Read) => {
                let read_data = match &operation.operation_data {
                    Some(OperationData::Read(data)) => data,
                    _ => return Err(anyhow::anyhow!("Read operation requires ReadOperationData")),
                };

                let read_cmd = ReadMessageCommand {
                    base: base_cmd,
                    message_ids: read_data.message_ids.clone(),
                    read_at: read_data.read_at.as_ref()
                        .map(|ts| chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32))
                        .flatten(),
                    burn_after_read: read_data.burn_after_read,
                };

                self.handle_read_message(read_cmd).await
            }
            Ok(OperationType::ReactionAdd) => {
                let reaction_data = match &operation.operation_data {
                    Some(OperationData::Reaction(data)) => data,
                    _ => return Err(anyhow::anyhow!("Reaction operation requires ReactionOperationData")),
                };

                let reaction_cmd = AddReactionCommand {
                    base: base_cmd,
                    emoji: reaction_data.emoji.clone(),
                };

                self.operation_service.handle_add_reaction(reaction_cmd).await?;
                Ok(())
            }
            Ok(OperationType::ReactionRemove) => {
                let reaction_data = match &operation.operation_data {
                    Some(OperationData::Reaction(data)) => data,
                    _ => return Err(anyhow::anyhow!("Reaction operation requires ReactionOperationData")),
                };

                let reaction_cmd = RemoveReactionCommand {
                    base: base_cmd,
                    emoji: reaction_data.emoji.clone(),
                };

                self.operation_service.handle_remove_reaction(reaction_cmd).await?;
                Ok(())
            }
            Ok(OperationType::Pin) => {
                let pin_data = match &operation.operation_data {
                    Some(OperationData::Pin(data)) => data,
                    _ => return Err(anyhow::anyhow!("Pin operation requires PinOperationData")),
                };

                let pin_cmd = PinMessageCommand {
                    base: base_cmd,
                    reason: if pin_data.reason.is_empty() {
                        None
                    } else {
                        Some(pin_data.reason.clone())
                    },
                    expire_at: pin_data.expire_at.as_ref().and_then(|ts| {
                        chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32)
                    }),
                };

                self.operation_service.handle_pin(pin_cmd).await?;
                Ok(())
            }
            Ok(OperationType::Unpin) => {
                let unpin_cmd = UnpinMessageCommand {
                    base: base_cmd,
                };

                self.operation_service.handle_unpin(unpin_cmd).await?;
                Ok(())
            }
            Ok(OperationType::Mark) => {
                let mark_data = match &operation.operation_data {
                    Some(OperationData::Mark(data)) => data,
                    _ => return Err(anyhow::anyhow!("Mark operation requires MarkOperationData")),
                };

                let mark_cmd = MarkMessageCommand {
                    base: base_cmd,
                    mark_type: mark_data.mark_type,
                    mark_value: if mark_data.color.is_empty() {
                        None
                    } else {
                        Some(mark_data.color.clone())
                    },
                };

                self.handle_mark_message(mark_cmd).await
            }
            Ok(OperationType::Unmark) => {
                let unmark_data = match &operation.operation_data {
                    Some(OperationData::Unmark(data)) => data,
                    _ => return Err(anyhow::anyhow!("Unmark operation requires UnmarkOperationData")),
                };

                let unmark_cmd = UnmarkMessageCommand {
                    base: base_cmd,
                    mark_type: if unmark_data.mark_type < 0 {
                        None
                    } else {
                        Some(unmark_data.mark_type)
                    },
                    user_id: operation.operator_id.clone(),
                };

                self.handle_unmark_message(unmark_cmd).await
            }
            _ => Err(anyhow::anyhow!("Unsupported operation type: {}", operation.operation_type)),
        }
    }
}
