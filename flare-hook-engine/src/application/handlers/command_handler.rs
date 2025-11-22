//! # Hook命令处理器（编排层）
//!
//! 负责处理命令，调用领域服务

use std::sync::Arc;

use anyhow::Result;

use crate::domain::model::HookExecutionPlan;
use crate::domain::service::HookOrchestrationService;
use flare_im_core::{
    DeliveryEvent, HookContext, MessageDraft, MessageRecord, PreSendDecision, RecallEvent,
};

/// Hook命令处理器（编排层）
pub struct HookCommandHandler {
    orchestration_service: Arc<HookOrchestrationService>,
}

impl HookCommandHandler {
    pub fn new(orchestration_service: Arc<HookOrchestrationService>) -> Self {
        Self {
            orchestration_service,
        }
    }

    /// 处理PreSend Hook命令
    pub async fn handle_pre_send(
        &self,
        ctx: &HookContext,
        draft: &mut MessageDraft,
        hooks: Vec<HookExecutionPlan>,
    ) -> Result<PreSendDecision> {
        self.orchestration_service
            .execute_pre_send(ctx, draft, hooks)
            .await
    }

    /// 处理PostSend Hook命令
    pub async fn handle_post_send(
        &self,
        ctx: &HookContext,
        record: &MessageRecord,
        draft: &MessageDraft,
        hooks: Vec<HookExecutionPlan>,
    ) -> Result<()> {
        self.orchestration_service
            .execute_post_send(ctx, record, draft, hooks)
            .await
    }

    /// 处理Delivery Hook命令
    pub async fn handle_delivery(
        &self,
        ctx: &HookContext,
        event: &DeliveryEvent,
        hooks: Vec<HookExecutionPlan>,
    ) -> Result<()> {
        self.orchestration_service
            .execute_delivery(ctx, event, hooks)
            .await
    }

    /// 处理Recall Hook命令
    pub async fn handle_recall(
        &self,
        ctx: &HookContext,
        event: &RecallEvent,
        hooks: Vec<HookExecutionPlan>,
    ) -> Result<PreSendDecision> {
        self.orchestration_service
            .execute_recall(ctx, event, hooks)
            .await
    }
}

