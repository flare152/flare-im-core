//! 命令处理器（编排层）- 轻量级，只负责编排领域服务

use std::sync::Arc;

use anyhow::Result;
use flare_proto::flare::push::v1::PushAckResponse;
use flare_proto::push::{PushMessageResponse, PushNotificationResponse};
use tracing::instrument;

use crate::application::commands::{
    EnqueueAckCommand, EnqueueMessageCommand, EnqueueNotificationCommand,
};
use crate::domain::service::PushDomainService;
use flare_im_core::hooks::HookDispatcher;

/// 推送命令处理器（编排层）
pub struct PushCommandHandler {
    domain_service: Arc<PushDomainService>,
    hook_dispatcher: HookDispatcher,
}

impl PushCommandHandler {
    pub fn new(domain_service: Arc<PushDomainService>, hook_dispatcher: HookDispatcher) -> Self {
        Self {
            domain_service,
            hook_dispatcher,
        }
    }

    /// 处理入队推送消息命令
    #[instrument(skip(self))]
    pub async fn handle_enqueue_message(
        &self,
        command: EnqueueMessageCommand,
    ) -> Result<PushMessageResponse> {
        self.domain_service.enqueue_message(command.request).await
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

    /// 处理入队 ACK 命令
    #[instrument(skip(self))]
    pub async fn handle_enqueue_ack(&self, command: EnqueueAckCommand) -> Result<PushAckResponse> {
        self.domain_service.enqueue_ack(command.request).await
    }
}
