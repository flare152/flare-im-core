//! 命令处理器（编排层）- 轻量级，只负责编排领域服务

use std::sync::Arc;

use anyhow::Result;
use flare_proto::push::{PushMessageResponse, PushNotificationResponse};
use tracing::instrument;

use crate::application::commands::{EnqueueMessageCommand, EnqueueNotificationCommand};
use crate::domain::service::PushDomainService;

/// 推送命令处理器（编排层）
pub struct PushCommandHandler {
    domain_service: Arc<PushDomainService>,
}

impl PushCommandHandler {
    pub fn new(domain_service: Arc<PushDomainService>) -> Self {
        Self { domain_service }
    }

    /// 处理入队推送消息命令
    #[instrument(skip(self))]
    pub async fn handle_enqueue_message(
        &self,
        command: EnqueueMessageCommand,
    ) -> Result<PushMessageResponse> {
        self.domain_service
            .enqueue_message(command.request)
            .await
    }

    /// 处理入队推送通知命令
    #[instrument(skip(self))]
    pub async fn handle_enqueue_notification(
        &self,
        command: EnqueueNotificationCommand,
    ) -> Result<PushNotificationResponse> {
        self.domain_service
            .enqueue_notification(command.request)
            .await
    }
}

