//! # Local Plugin适配器
//!
//! 提供基于本地插件的Hook传输适配器实现。

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;

use flare_im_core::{
    DeliveryEvent, DeliveryHook, HookContext, MessageDraft, MessageRecord, PostSendHook,
    PreSendDecision, PreSendHook, RecallEvent, RecallHook,
};

/// Local Plugin适配器
pub struct LocalHookAdapter {
    pre_send_hooks: HashMap<String, Arc<dyn PreSendHook>>,
    post_send_hooks: HashMap<String, Arc<dyn PostSendHook>>,
    delivery_hooks: HashMap<String, Arc<dyn DeliveryHook>>,
    recall_hooks: HashMap<String, Arc<dyn RecallHook>>,
}

impl LocalHookAdapter {
    /// 创建Local Plugin适配器
    pub fn new(_target: String) -> anyhow::Result<Self> {
        Ok(Self {
            pre_send_hooks: HashMap::new(),
            post_send_hooks: HashMap::new(),
            delivery_hooks: HashMap::new(),
            recall_hooks: HashMap::new(),
        })
    }
    
    /// 注册PreSend Hook
    pub fn register_pre_send(&mut self, name: String, hook: Arc<dyn PreSendHook>) {
        self.pre_send_hooks.insert(name, hook);
    }
    
    /// 注册PostSend Hook
    pub fn register_post_send(&mut self, name: String, hook: Arc<dyn PostSendHook>) {
        self.post_send_hooks.insert(name, hook);
    }
    
    /// 注册Delivery Hook
    pub fn register_delivery(&mut self, name: String, hook: Arc<dyn DeliveryHook>) {
        self.delivery_hooks.insert(name, hook);
    }
    
    /// 注册Recall Hook
    pub fn register_recall(&mut self, name: String, hook: Arc<dyn RecallHook>) {
        self.recall_hooks.insert(name, hook);
    }
    
    /// 执行PreSend Hook
    pub async fn pre_send(
        &self,
        target: &str,
        ctx: &HookContext,
        draft: &mut MessageDraft,
    ) -> Result<PreSendDecision> {
        let hook = self.pre_send_hooks.get(target)
            .ok_or_else(|| anyhow::anyhow!("Local PreSend hook not found: {}", target))?;
        
        Ok(hook.handle(ctx, draft).await)
    }
    
    /// 执行PostSend Hook
    pub async fn post_send(
        &self,
        target: &str,
        ctx: &HookContext,
        record: &MessageRecord,
        draft: &MessageDraft,
    ) -> Result<()> {
        let hook = self.post_send_hooks.get(target)
            .ok_or_else(|| anyhow::anyhow!("Local PostSend hook not found: {}", target))?;
        
        let outcome = hook.handle(ctx, record, draft).await;
        if outcome.is_completed() {
            Ok(())
        } else {
            anyhow::bail!("PostSend hook failed")
        }
    }
    
    /// 执行Delivery Hook
    pub async fn delivery(
        &self,
        target: &str,
        ctx: &HookContext,
        event: &DeliveryEvent,
    ) -> Result<()> {
        let hook = self.delivery_hooks.get(target)
            .ok_or_else(|| anyhow::anyhow!("Local Delivery hook not found: {}", target))?;
        
        let outcome = hook.handle(ctx, event).await;
        if outcome.is_completed() {
            Ok(())
        } else {
            anyhow::bail!("Delivery hook failed")
        }
    }
    
    /// 执行Recall Hook
    pub async fn recall(
        &self,
        target: &str,
        ctx: &HookContext,
        event: &RecallEvent,
    ) -> Result<PreSendDecision> {
        let hook = self.recall_hooks.get(target)
            .ok_or_else(|| anyhow::anyhow!("Local Recall hook not found: {}", target))?;
        
        let outcome = hook.handle(ctx, event).await;
        if outcome.is_completed() {
            Ok(PreSendDecision::Continue)
        } else {
            use flare_im_core::error::{ErrorBuilder, ErrorCode};
            let error = ErrorBuilder::new(ErrorCode::OperationFailed, "Recall hook failed")
                .build_error();
            Ok(PreSendDecision::Reject { error })
        }
    }
}

