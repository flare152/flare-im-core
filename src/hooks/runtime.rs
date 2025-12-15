use std::sync::Arc;

use crate::error::Result;

use super::registry::HookRegistry;
use super::types::{DeliveryEvent, GetSessionParticipantsHook, HookContext, MessageDraft, MessageRecord, RecallEvent};

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

    /// 检查Hook系统是否可用
    pub fn is_available(&self) -> bool {
        // Hook系统总是可用的，因为我们有注册表
        true
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

    /// 调用获取会话参与者的Hook
    pub async fn invoke_get_session_participants(
        &self,
        ctx: &HookContext,
        session_id: &str,
    ) -> Result<Option<Vec<String>>> {
        // 在Hook注册表中查找GetSessionParticipantsHook类型的Hook
        // 这里简化实现，实际应该遍历注册表查找合适的Hook
        Ok(None)
    }
}
