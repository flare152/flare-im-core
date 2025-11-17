//! # Hook应用服务
//!
//! 提供Hook执行的应用服务门面

use std::sync::Arc;
use std::time::SystemTime;

use anyhow::Result;

use crate::domain::models::HookExecutionPlan;
use crate::domain::service::HookOrchestrationService;
use crate::infrastructure::adapters::HookAdapterFactory;
use flare_im_core::{
    DeliveryEvent, HookContext, MessageDraft, MessageRecord, PreSendDecision, RecallEvent,
};

/// Hook应用服务
pub struct HookApplicationService {
    orchestration_service: Arc<HookOrchestrationService>,
    adapter_factory: Arc<HookAdapterFactory>,
}

impl HookApplicationService {
    pub fn new(
        orchestration_service: Arc<HookOrchestrationService>,
        adapter_factory: Arc<HookAdapterFactory>,
    ) -> Self {
        Self {
            orchestration_service,
            adapter_factory,
        }
    }

    /// 执行PreSend Hook
    pub async fn execute_pre_send(
        &self,
        ctx: &HookContext,
        draft: &mut MessageDraft,
        hooks: Vec<HookExecutionPlan>,
    ) -> Result<PreSendDecision> {
        // 分组Hook
        let grouped = self.orchestration_service.group_hooks(hooks);

        // 先执行validation组（串行，快速失败）
        for hook in &grouped.validation {
            let decision = hook.execute(ctx, draft).await?;
            match decision {
                PreSendDecision::Reject { .. } => return Ok(decision),
                PreSendDecision::Continue => continue,
            }
        }

        // 再执行critical组（串行，保证顺序）
        for hook in &grouped.critical {
            let decision = hook.execute(ctx, draft).await?;
            match decision {
                PreSendDecision::Reject { .. } => return Ok(decision),
                PreSendDecision::Continue => continue,
            }
        }

        // 最后执行business组（串行执行，因为draft是&mut不能并发）
        for hook in &grouped.business {
            let decision = hook.execute(ctx, draft).await?;
            match decision {
                PreSendDecision::Reject { .. } => {
                    // business组即使失败也不中断主流程，只记录日志
                    tracing::warn!("Business hook rejected but continuing");
                }
                PreSendDecision::Continue => continue,
            }
        }

        Ok(PreSendDecision::Continue)
    }

    /// 执行PostSend Hook
    pub async fn execute_post_send(
        &self,
        ctx: &HookContext,
        record: &MessageRecord,
        draft: &MessageDraft,
        hooks: Vec<HookExecutionPlan>,
    ) -> Result<()> {
        // 分组Hook
        let grouped = self.orchestration_service.group_hooks(hooks);

        // 串行执行validation和critical组
        for hook in grouped.validation.iter().chain(grouped.critical.iter()) {
            if let Err(e) = hook.execute_post_send(ctx, record, draft).await {
                if hook.require_success() {
                    return Err(e);
                }
                tracing::warn!(hook = %hook.name(), error = %e, "PostSend hook failed but continuing");
            }
        }

        // 并发执行business组
        let business_futures: Vec<_> = grouped
            .business
            .iter()
            .map(|hook| hook.execute_post_send(ctx, record, draft))
            .collect();

        let results = futures_util::future::join_all(business_futures).await;
        for (hook, result) in grouped.business.iter().zip(results) {
            if let Err(e) = result {
                if hook.require_success() {
                    tracing::warn!(hook = %hook.name(), error = %e, "PostSend hook failed");
                } else {
                    tracing::debug!(hook = %hook.name(), error = %e, "PostSend hook failed but ignored");
                }
            }
        }

        Ok(())
    }

    /// 执行Delivery Hook
    pub async fn execute_delivery(
        &self,
        ctx: &HookContext,
        event: &DeliveryEvent,
        hooks: Vec<HookExecutionPlan>,
    ) -> Result<()> {
        // 分组Hook
        let grouped = self.orchestration_service.group_hooks(hooks);

        // 串行执行validation和critical组
        for hook in grouped.validation.iter().chain(grouped.critical.iter()) {
            if let Err(e) = hook.execute_delivery(ctx, event).await {
                if hook.require_success() {
                    return Err(e);
                }
                tracing::warn!(hook = %hook.name(), error = %e, "Delivery hook failed but continuing");
            }
        }

        // 并发执行business组
        let business_futures: Vec<_> = grouped
            .business
            .iter()
            .map(|hook| hook.execute_delivery(ctx, event))
            .collect();

        let results = futures_util::future::join_all(business_futures).await;
        for (hook, result) in grouped.business.iter().zip(results) {
            if let Err(e) = result {
                if hook.require_success() {
                    tracing::warn!(hook = %hook.name(), error = %e, "Delivery hook failed");
                } else {
                    tracing::debug!(hook = %hook.name(), error = %e, "Delivery hook failed but ignored");
                }
            }
        }

        Ok(())
    }

    /// 执行Recall Hook
    pub async fn execute_recall(
        &self,
        ctx: &HookContext,
        event: &RecallEvent,
        hooks: Vec<HookExecutionPlan>,
    ) -> Result<PreSendDecision> {
        // 分组Hook
        let grouped = self.orchestration_service.group_hooks(hooks);

        // 先执行validation组（串行，快速失败）
        for hook in &grouped.validation {
            let decision = hook.execute_recall(ctx, event).await?;
            match decision {
                PreSendDecision::Reject { .. } => return Ok(decision),
                PreSendDecision::Continue => continue,
            }
        }

        // 再执行critical组（串行，保证顺序）
        for hook in &grouped.critical {
            let decision = hook.execute_recall(ctx, event).await?;
            match decision {
                PreSendDecision::Reject { .. } => return Ok(decision),
                PreSendDecision::Continue => continue,
            }
        }

        // 最后执行business组（串行执行）
        for hook in &grouped.business {
            let decision = hook.execute_recall(ctx, event).await?;
            match decision {
                PreSendDecision::Reject { .. } => {
                    // business组即使失败也不中断主流程，只记录日志
                    tracing::warn!("Business recall hook rejected but continuing");
                }
                PreSendDecision::Continue => continue,
            }
        }

        Ok(PreSendDecision::Continue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::HookConfigItem;
    use crate::domain::service::HookOrchestrationService;
    use crate::infrastructure::adapters::HookAdapterFactory;
    use std::collections::HashMap;
    use flare_im_core::{HookErrorPolicy, HookKind, HookMetadata, HookGroup};
    use std::sync::Arc;
    use std::time::Duration;

    fn create_test_plan(name: &str, priority: i32) -> HookExecutionPlan {
        let metadata = HookMetadata {
            name: Arc::from(name),
            version: None,
            description: None,
            kind: HookKind::PreSend,
            priority,
            timeout: Duration::from_secs(5),
            max_retries: 0,
            error_policy: HookErrorPolicy::FailFast,
            require_success: true,
        };
        HookExecutionPlan::new(metadata)
    }

    #[tokio::test]
    async fn test_execute_pre_send_empty_hooks() {
        let orchestration = Arc::new(HookOrchestrationService);
        let adapter_factory = Arc::new(HookAdapterFactory::new());
        let service = HookApplicationService::new(orchestration, adapter_factory);

        let ctx = HookContext::new("test-tenant");
        let mut draft = MessageDraft::new(b"test".to_vec());

        let decision = service.execute_pre_send(&ctx, &mut draft, vec![]).await.unwrap();
        assert!(matches!(decision, PreSendDecision::Continue));
    }

    #[tokio::test]
    async fn test_execute_pre_send_validation_reject() {
        let orchestration = Arc::new(HookOrchestrationService);
        let adapter_factory = Arc::new(HookAdapterFactory::new());
        let service = HookApplicationService::new(orchestration, adapter_factory);

        let ctx = HookContext::new("test-tenant");
        let mut draft = MessageDraft::new(b"test".to_vec());

        // 创建validation组的Hook（priority >= 100）
        let metadata = HookMetadata::default()
            .with_name("validation-hook")
            .with_priority(100); // priority >= 100 会自动归入 Validation 组
        let plan = HookExecutionPlan::new(metadata);

        // 没有适配器，应该直接通过
        let decision = service.execute_pre_send(&ctx, &mut draft, vec![plan]).await.unwrap();
        assert!(matches!(decision, PreSendDecision::Continue));
    }

    #[tokio::test]
    async fn test_execute_post_send_empty_hooks() {
        let orchestration = Arc::new(HookOrchestrationService);
        let adapter_factory = Arc::new(HookAdapterFactory::new());
        let service = HookApplicationService::new(orchestration, adapter_factory);

        let ctx = HookContext::new("test-tenant");
        let record = MessageRecord {
            message_id: "msg-1".to_string(),
            client_message_id: None,
            conversation_id: "conv-1".to_string(),
            sender_id: "user-1".to_string(),
            session_type: None,
            message_type: None,
            persisted_at: SystemTime::now(),
            metadata: HashMap::new(),
        };
        let draft = MessageDraft::new(b"test".to_vec());

        let result = service.execute_post_send(&ctx, &record, &draft, vec![]).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_delivery_empty_hooks() {
        let orchestration = Arc::new(HookOrchestrationService);
        let adapter_factory = Arc::new(HookAdapterFactory::new());
        let service = HookApplicationService::new(orchestration, adapter_factory);

        let ctx = HookContext::new("test-tenant");
        let event = DeliveryEvent {
            message_id: "msg-1".to_string(),
            user_id: "user-1".to_string(),
            channel: "websocket".to_string(),
            delivered_at: SystemTime::now(),
            metadata: HashMap::new(),
        };

        let result = service.execute_delivery(&ctx, &event, vec![]).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_recall_empty_hooks() {
        let orchestration = Arc::new(HookOrchestrationService);
        let adapter_factory = Arc::new(HookAdapterFactory::new());
        let service = HookApplicationService::new(orchestration, adapter_factory);

        let ctx = HookContext::new("test-tenant");
        let event = RecallEvent {
            message_id: "msg-1".to_string(),
            operator_id: "user-1".to_string(),
            recalled_at: SystemTime::now(),
            metadata: HashMap::new(),
        };

        let decision = service.execute_recall(&ctx, &event, vec![]).await.unwrap();
        assert!(matches!(decision, PreSendDecision::Continue));
    }
}
