//! 命令处理器（编排层）- 轻量级，只负责编排领域服务

use std::sync::Arc;

use flare_server_core::error::Result;
use tracing::instrument;

use crate::application::commands::{BatchExecutePushTasksCommand, ExecutePushTaskCommand};
use crate::domain::service::PushDomainService;

/// 推送命令处理器（编排层）
pub struct PushCommandHandler {
    domain_service: Arc<PushDomainService>,
}

impl PushCommandHandler {
    pub fn new(domain_service: Arc<PushDomainService>) -> Self {
        Self { domain_service }
    }

    /// 处理执行推送任务命令
    #[instrument(skip(self), fields(user_id = %command.task.user_id, message_id = %command.task.message_id))]
    pub async fn handle_execute_push_task(&self, command: ExecutePushTaskCommand) -> Result<()> {
        self.domain_service.execute_push_task(command.task).await
    }

    /// 处理批量执行推送任务命令
    #[instrument(skip(self), fields(batch_size = command.tasks.len()))]
    pub async fn handle_batch_execute_push_tasks(
        &self,
        command: BatchExecutePushTasksCommand,
    ) -> Result<()> {
        self.domain_service
            .execute_push_tasks_batch(command.tasks)
            .await
    }
}
