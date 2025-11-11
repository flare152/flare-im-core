use std::sync::Arc;

use crate::error::Result;

use super::registry::HookRegistry;
use super::types::{DeliveryEvent, HookContext, MessageDraft, MessageRecord, RecallEvent};

/// Hook 调度器，封装常用执行入口
#[derive(Clone)]
pub struct HookDispatcher {
    registry: Arc<HookRegistry>,
}

impl HookDispatcher {
    pub fn new(registry: Arc<HookRegistry>) -> Self {
        Self { registry }
    }

    pub fn registry(&self) -> &Arc<HookRegistry> {
        &self.registry
    }

    pub async fn pre_send(&self, ctx: &HookContext, draft: &mut MessageDraft) -> Result<()> {
        self.registry.execute_pre_send(ctx, draft).await
    }

    pub async fn post_send(
        &self,
        ctx: &HookContext,
        record: &MessageRecord,
        draft: &MessageDraft,
    ) -> Result<()> {
        self.registry.execute_post_send(ctx, record, draft).await
    }

    pub async fn delivery(&self, ctx: &HookContext, event: &DeliveryEvent) -> Result<()> {
        self.registry.execute_delivery(ctx, event).await
    }

    pub async fn recall(&self, ctx: &HookContext, event: &RecallEvent) -> Result<()> {
        self.registry.execute_recall(ctx, event).await
    }
}
