//! 命令处理器（编排层）- 轻量级，只负责编排领域服务

use std::sync::Arc;
use anyhow::Result;
use tracing::instrument;
use prost_types::Timestamp;

use crate::application::commands::{
    ClearSessionCommand, DeleteMessageCommand, DeleteMessageForUserCommand,
    ExportMessagesCommand, MarkReadCommand, RecallMessageCommand, SetMessageAttributesCommand,
};
use crate::domain::service::MessageStorageDomainService;

/// 消息存储命令处理器（编排层）
pub struct MessageStorageCommandHandler {
    domain_service: Arc<MessageStorageDomainService>,
}

impl MessageStorageCommandHandler {
    pub fn new(domain_service: Arc<MessageStorageDomainService>) -> Self {
        Self { domain_service }
    }

    /// 删除消息
    #[instrument(skip(self), fields(message_count = command.message_ids.len()))]
    pub async fn handle_delete_message(
        &self,
        command: DeleteMessageCommand,
    ) -> Result<usize> {
        self.domain_service.delete_messages(&command.message_ids).await
    }

    /// 撤回消息
    #[instrument(skip(self), fields(message_id = %command.message_id))]
    pub async fn handle_recall_message(
        &self,
        command: RecallMessageCommand,
    ) -> Result<Option<Timestamp>> {
        self.domain_service
            .recall_message(&command.message_id, command.recall_time_limit_seconds)
            .await
    }

    /// 标记消息已读
    #[instrument(skip(self), fields(message_id = %command.message_id, user_id = %command.user_id))]
    pub async fn handle_mark_read(
        &self,
        command: MarkReadCommand,
    ) -> Result<(Timestamp, Option<Timestamp>)> {
        self.domain_service
            .mark_message_read(&command.message_id, &command.user_id)
            .await
    }

    /// 为用户删除消息
    #[instrument(skip(self), fields(message_id = %command.message_ids.first().map(|s| s.as_str()).unwrap_or(""), user_id = %command.user_id))]
    pub async fn handle_delete_message_for_user(
        &self,
        command: DeleteMessageForUserCommand,
    ) -> Result<usize> {
        let mut total_deleted = 0;
        for message_id in &command.message_ids {
            let deleted = self.domain_service
                .delete_message_for_user(message_id, &command.user_id, command.permanent)
                .await?;
            total_deleted += deleted;
        }
        Ok(total_deleted)
    }

    /// 设置消息属性
    #[instrument(skip(self), fields(message_id = %command.message_id))]
    pub async fn handle_set_message_attributes(
        &self,
        command: SetMessageAttributesCommand,
    ) -> Result<()> {
        self.domain_service
            .set_message_attributes(&command.message_id, command.attributes, command.tags)
            .await
    }

    /// 设置消息属性并追加一条操作审计记录
    #[instrument(skip(self), fields(message_id = %command.message_id, operation_type = %operation.operation_type))]
    pub async fn handle_set_attributes_with_operation(
        &self,
        command: SetMessageAttributesCommand,
        operation: flare_proto::common::MessageOperation,
    ) -> Result<()> {
        self.domain_service
            .append_operation_and_attributes(&command.message_id, operation, command.attributes, command.tags)
            .await
    }

    /// 添加或移除反应
    #[instrument(skip(self), fields(message_id = %message_id, emoji = %emoji, user_id = %user_id))]
    pub async fn handle_add_or_remove_reaction(
        &self,
        message_id: &str,
        emoji: &str,
        user_id: &str,
        is_add: bool,
    ) -> Result<Vec<flare_proto::common::Reaction>> {
        self.domain_service
            .add_or_remove_reaction(message_id, emoji, user_id, is_add)
            .await
    }

    /// 获取 domain_service（用于直接访问领域服务）
    pub fn domain_service(&self) -> &Arc<MessageStorageDomainService> {
        &self.domain_service
    }

    /// 清理会话
    #[instrument(skip(self), fields(session_id = %command.session_id))]
    pub async fn handle_clear_session(
        &self,
        command: ClearSessionCommand,
    ) -> Result<usize> {
        let user_id = command.user_id.as_deref().unwrap_or("");
        self.domain_service
            .clear_session(&command.session_id, user_id, command.clear_before_time)
            .await
    }

    /// 导出消息（异步任务，返回任务ID）
    #[instrument(skip(self), fields(session_id = %command.session_id))]
    pub async fn handle_export_messages(
        &self,
        command: ExportMessagesCommand,
    ) -> Result<String> {
        // TODO: 实现异步导出任务
        // 当前简化实现：直接返回任务ID
        use uuid::Uuid;
        let export_task_id = format!("export-{}", Uuid::new_v4());
        Ok(export_task_id)
    }
}
