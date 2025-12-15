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
        // 生成唯一的导出任务ID
        use uuid::Uuid;
        let export_task_id = format!("export-{}", Uuid::new_v4());
        
        // 克隆必要的依赖项
        let domain_service = self.domain_service.clone();
        let command_clone = command.clone();
        let export_task_id_clone = export_task_id.clone();
        
        // 异步执行导出任务
        tokio::spawn(async move {
            // 执行导出逻辑
            if let Err(e) = Self::execute_export_task(domain_service, command_clone, &export_task_id_clone).await {
                tracing::error!(
                    task_id = %export_task_id_clone,
                    error = %e,
                    "Export task failed"
                );
            } else {
                tracing::info!(
                    task_id = %export_task_id_clone,
                    "Export task completed successfully"
                );
            }
        });
        
        Ok(export_task_id)
    }
    
    /// 执行导出任务的具体逻辑
    async fn execute_export_task(
        domain_service: Arc<MessageStorageDomainService>,
        command: ExportMessagesCommand,
        task_id: &str,
    ) -> Result<()> {
        tracing::info!(
            task_id = %task_id,
            session_id = %command.session_id,
            "Starting export task"
        );
        
        // 查询消息
        let messages = domain_service
            .query_messages(
                &command.session_id,
                command.start_time.unwrap_or(0),
                command.end_time.unwrap_or(0),
                command.limit.unwrap_or(100),
                None,
            )
            .await?;
        
        // 这里应该实现实际的导出逻辑，比如：
        // 1. 将消息序列化为某种格式（CSV、JSON等）
        // 2. 上传到对象存储（S3、MinIO等）
        // 3. 通知用户导出完成
        
        // 简化实现：只是记录导出的消息数量
        tracing::info!(
            task_id = %task_id,
            session_id = %command.session_id,
            message_count = messages.messages.len(),
            "Exported messages"
        );
        
        // 模拟一些处理时间
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        
        Ok(())
    }
}
