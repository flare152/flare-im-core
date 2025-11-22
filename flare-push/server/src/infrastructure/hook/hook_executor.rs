//! Hook 执行器 - 使用 flare-hook-engine 执行 Hook

use std::sync::Arc;

// Hook Engine 暂时注释，等待实现
// use flare_hook_engine::service::HookEngine;
use flare_im_core::hooks::{HookContext, MessageDraft, MessageRecord};
use flare_server_core::error::Result;
use tracing::instrument;

/// Hook 执行器 - 封装 flare-hook-engine
/// TODO: 等待 flare-hook-engine 实现后启用
pub struct HookExecutor {
    // engine: Arc<HookEngine>,
}

impl HookExecutor {
    pub fn new(_engine: Arc<()>) -> Self {
        Self {
            // engine,
        }
    }

    /// 执行 PreSend Hook
    #[instrument(skip(self))]
    pub async fn pre_send(
        &self,
        _ctx: &HookContext,
        _draft: &mut MessageDraft,
    ) -> Result<()> {
        // TODO: 实现 Hook 执行逻辑
        // match self.engine.command_service.pre_send(ctx, draft).await {
        //     Ok(decision) => {
        //         if decision.is_rejected() {
        //             return Err(flare_server_core::error::ErrorBuilder::new(
        //                 flare_server_core::error::ErrorCode::InvalidParameter,
        //                 "Hook rejected message",
        //             )
        //             .build_error());
        //         }
        //         Ok(())
        //     }
        //     Err(e) => {
        //         warn!(error = %e, "Hook execution failed");
        //         Ok(())
        //     }
        // }
        Ok(())
    }

    /// 执行 PostSend Hook
    #[instrument(skip(self, _ctx, _record, _draft))]
    pub async fn post_send(
        &self,
        _ctx: &HookContext,
        _record: &MessageRecord,
        _draft: &MessageDraft,
    ) -> Result<()> {
        // TODO: 实现 Hook 执行逻辑
        // if let Err(e) = self.engine.command_service.post_send(ctx, record, draft).await {
        //     warn!(error = %e, "PostSend hook execution failed");
        // }
        Ok(())
    }
}

