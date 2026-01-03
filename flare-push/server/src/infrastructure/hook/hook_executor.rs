//! Hook 执行器 - 使用 flare-hook-engine 执行 Hook

use std::sync::Arc;
use std::time::SystemTime;

use flare_hook_engine::interface::grpc::HookExtensionServer;
use flare_im_core::hooks::{MessageDraft, MessageRecord};
use flare_server_core::context::{Context, ContextExt};
use flare_proto::hooks::hook_extension_server::HookExtension;
use flare_server_core::error::Result;
use tonic::IntoRequest;
use tracing::instrument;

/// Hook 执行器 - 封装 flare-hook-engine
pub struct HookExecutor {
    hook_extension_service: Arc<HookExtensionServer>,
}

impl HookExecutor {
    pub fn new(hook_extension_service: Arc<HookExtensionServer>) -> Self {
        Self {
            hook_extension_service,
        }
    }

    /// 执行 PreSend Hook
    #[instrument(skip(self, ctx), fields(
        request_id = %ctx.request_id(),
        trace_id = %ctx.trace_id(),
    ))]
    pub async fn pre_send(&self, ctx: &Context, draft: &mut MessageDraft) -> Result<()> {
        ctx.ensure_not_cancelled().map_err(|e| {
            flare_server_core::error::ErrorBuilder::new(
                flare_server_core::error::ErrorCode::InternalError,
                "Request cancelled",
            )
            .details(e.to_string())
            .build_error()
        })?;
        use flare_im_core::hooks::hook_context_data::get_hook_context_data;
        use flare_hook_engine::infrastructure::adapters::conversion::context_to_proto;
        
        // 构建PreSendHookRequest
        let request = flare_proto::hooks::PreSendHookRequest {
            context: Some(context_to_proto(ctx)),
            draft: Some(flare_proto::hooks::HookMessageDraft {
                message_id: draft.message_id.clone().unwrap_or_default(),
                client_message_id: draft.client_message_id.clone().unwrap_or_default(),
                conversation_id: draft.conversation_id.clone().unwrap_or_default(),
                payload: draft.payload.clone(),
                headers: draft.headers.clone(),
                metadata: draft.metadata.clone(),
                ..Default::default()
            }),
            ..Default::default()
        };

        // 调用Hook引擎执行PreSend Hook
        match self
            .hook_extension_service
            .invoke_pre_send(request.into_request())
            .await
        {
            Ok(response) => {
                let resp = response.into_inner();
                if !resp.allow {
                    return Err(flare_server_core::error::ErrorBuilder::new(
                        flare_server_core::error::ErrorCode::InvalidParameter,
                        "Hook rejected message",
                    )
                    .build_error());
                }

                // 如果draft被修改，更新原始draft
                if let Some(modified_draft) = resp.draft {
                    draft.message_id = if !modified_draft.message_id.is_empty() {
                        Some(modified_draft.message_id)
                    } else {
                        draft.message_id.clone()
                    };
                    draft.client_message_id = if !modified_draft.client_message_id.is_empty() {
                        Some(modified_draft.client_message_id)
                    } else {
                        draft.client_message_id.clone()
                    };
                    draft.conversation_id = if !modified_draft.conversation_id.is_empty() {
                        Some(modified_draft.conversation_id)
                    } else {
                        draft.conversation_id.clone()
                    };
                    draft.payload = modified_draft.payload;
                    draft.headers = modified_draft.headers;
                    draft.metadata = modified_draft.metadata;
                }

                Ok(())
            }
            Err(e) => {
                tracing::warn!(error = %e, "Hook execution failed");
                Ok(())
            }
        }
    }

    /// 执行 PostSend Hook
    #[instrument(skip(self, ctx, record, draft), fields(
        request_id = %ctx.request_id(),
        trace_id = %ctx.trace_id(),
    ))]
    pub async fn post_send(
        &self,
        ctx: &Context,
        record: &MessageRecord,
        draft: &MessageDraft,
    ) -> Result<()> {
        ctx.ensure_not_cancelled().map_err(|e| {
            flare_server_core::error::ErrorBuilder::new(
                flare_server_core::error::ErrorCode::InternalError,
                "Request cancelled",
            )
            .details(e.to_string())
            .build_error()
        })?;
        use flare_hook_engine::infrastructure::adapters::conversion::context_to_proto;
        
        // 构建PostSendHookRequest
        let request = flare_proto::hooks::PushPostSendHookRequest {
            context: Some(context_to_proto(ctx)),
            record: Some(flare_proto::hooks::HookPushRecord {
                task_id: record.message_id.clone(),
                user_id: record.sender_id.clone(),
                title: String::new(),   // 根据实际需求填充
                content: String::new(), // 根据实际需求填充
                channel: "push".to_string(),
                enqueued_at: Some(prost_types::Timestamp {
                    seconds: record
                        .persisted_at
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0),
                    nanos: 0,
                }),
                metadata: record.metadata.clone(),
            }),
            draft: Some(flare_proto::hooks::HookPushDraft {
                task_id: draft.message_id.clone().unwrap_or_default(),
                user_id: String::new(), // 根据实际需求填充
                title: String::new(),   // 根据实际需求填充
                content: String::from_utf8_lossy(&draft.payload).to_string(),
                channel: "push".to_string(),
                message_id: draft.message_id.clone().unwrap_or_default(),
                metadata: draft.metadata.clone(),
            }),
            ..Default::default()
        };

        // 调用Hook引擎执行PostSend Hook
        if let Err(e) = self
            .hook_extension_service
            .invoke_push_post_send(request.into_request())
            .await
        {
            tracing::warn!(error = %e, "PostSend hook execution failed");
        }
        Ok(())
    }
}
