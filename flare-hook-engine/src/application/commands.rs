//! # Hook命令服务
//!
//! 提供Hook执行的命令接口

use std::sync::Arc;

use anyhow::Result;

use crate::application::service::HookApplicationService;
use crate::domain::models::HookExecutionPlan;
use flare_im_core::{
    DeliveryEvent, HookContext, MessageDraft, MessageRecord, PreSendDecision, RecallEvent,
};

/// Hook命令服务
pub struct HookCommandService {
    application_service: Arc<HookApplicationService>,
}

impl HookCommandService {
    pub fn new(application_service: Arc<HookApplicationService>) -> Self {
        Self {
            application_service,
        }
    }

    /// 执行PreSend Hook
    pub async fn pre_send(
        &self,
        ctx: &HookContext,
        draft: &mut MessageDraft,
        hooks: Vec<HookExecutionPlan>,
    ) -> Result<PreSendDecision> {
        self.application_service
            .execute_pre_send(ctx, draft, hooks)
            .await
    }

    /// 执行PostSend Hook
    pub async fn post_send(
        &self,
        ctx: &HookContext,
        record: &MessageRecord,
        draft: &MessageDraft,
        hooks: Vec<HookExecutionPlan>,
    ) -> Result<()> {
        self.application_service
            .execute_post_send(ctx, record, draft, hooks)
            .await
    }

    /// 执行Delivery Hook
    pub async fn delivery(
        &self,
        ctx: &HookContext,
        event: &DeliveryEvent,
        hooks: Vec<HookExecutionPlan>,
    ) -> Result<()> {
        self.application_service
            .execute_delivery(ctx, event, hooks)
            .await
    }

    /// 执行Recall Hook
    pub async fn recall(
        &self,
        ctx: &HookContext,
        event: &RecallEvent,
        hooks: Vec<HookExecutionPlan>,
    ) -> Result<PreSendDecision> {
        self.application_service
            .execute_recall(ctx, event, hooks)
            .await
    }
}

