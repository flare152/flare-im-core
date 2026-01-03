//! 命令处理器（编排层）- 轻量级，只负责编排领域服务

use std::sync::Arc;

use anyhow::Result;
use flare_proto::flare::push::v1::PushAckResponse;
use flare_proto::push::{PushMessageResponse, PushNotificationResponse};
use flare_server_core::context::{Context, ContextExt};
use tracing::instrument;

use crate::application::commands::{
    EnqueueAckCommand, EnqueueMessageCommand, EnqueueNotificationCommand,
};
use crate::domain::service::PushDomainService;

/// 推送命令处理器（编排层）
pub struct PushCommandHandler {
    domain_service: Arc<PushDomainService>,
}

impl PushCommandHandler {
    pub fn new(domain_service: Arc<PushDomainService>) -> Self {
        Self {
            domain_service,
        }
    }

    /// 处理入队推送消息命令
    #[instrument(skip(self, ctx), fields(
        request_id = %ctx.request_id(),
        trace_id = %ctx.trace_id(),
    ))]
    pub async fn handle_enqueue_message(
        &self,
        ctx: &Context,
        command: EnqueueMessageCommand,
    ) -> Result<PushMessageResponse> {
        ctx.ensure_not_cancelled()?;
        self.domain_service.enqueue_message(ctx, command.request).await
    }

    /// 处理入队推送通知命令
    #[instrument(skip(self, ctx), fields(
        request_id = %ctx.request_id(),
        trace_id = %ctx.trace_id(),
    ))]
    pub async fn handle_enqueue_notification(
        &self,
        ctx: &Context,
        command: EnqueueNotificationCommand,
    ) -> Result<PushNotificationResponse> {
        ctx.ensure_not_cancelled()?;
        self.domain_service
            .enqueue_notification(ctx, command.request)
            .await
    }

    /// 处理入队 ACK 命令
    #[instrument(skip(self, ctx), fields(
        request_id = %ctx.request_id(),
        trace_id = %ctx.trace_id(),
    ))]
    pub async fn handle_enqueue_ack(&self, ctx: &Context, command: EnqueueAckCommand) -> Result<PushAckResponse> {
        ctx.ensure_not_cancelled()?;
        self.domain_service.enqueue_ack(ctx, command.request).await
    }
}
