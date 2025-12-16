//! 命令处理器（编排层）- 轻量级，只负责编排领域服务

use std::sync::Arc;

use flare_server_core::error::Result;
use tracing::instrument;

use crate::application::commands::{
    BatchPushTasksCommand, PushMessageCommand, PushNotificationCommand,
};
use crate::domain::service::PushDomainService;

/// 推送命令处理器（编排层）
pub struct PushCommandHandler {
    domain_service: Arc<PushDomainService>,
}

impl PushCommandHandler {
    pub fn new(domain_service: Arc<PushDomainService>) -> Self {
        Self { domain_service }
    }

    /// 处理推送消息命令
    #[instrument(skip(self))]
    pub async fn handle_push_message(&self, command: PushMessageCommand) -> Result<()> {
        self.domain_service
            .dispatch_push_message(command.request)
            .await
    }

    /// 处理推送通知命令
    #[instrument(skip(self))]
    pub async fn handle_push_notification(&self, command: PushNotificationCommand) -> Result<()> {
        self.domain_service
            .dispatch_push_notification(command.request)
            .await
    }

    /// 处理批量推送任务命令
    #[instrument(skip(self), fields(batch_size = command.batch.len()))]
    pub async fn handle_batch_push_tasks(&self, command: BatchPushTasksCommand) -> Result<()> {
        self.domain_service
            .process_push_tasks_batch(command.batch)
            .await
    }
}
