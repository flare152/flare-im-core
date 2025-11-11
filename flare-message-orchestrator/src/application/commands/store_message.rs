use std::sync::Arc;

use flare_im_core::error::{ErrorBuilder, ErrorCode, Result};
use flare_im_core::hooks::HookDispatcher;
use flare_proto::storage::StoreMessageRequest;

use crate::application::hooks::{
    apply_draft_to_request, build_draft_from_request, build_hook_context, build_message_record,
    draft_from_submission, merge_context,
};
use crate::domain::message_submission::{MessageDefaults, MessageSubmission};
use crate::domain::repositories::{MessageEventPublisher, WalRepository};

/// 命令对象，封装单条存储请求。
pub struct StoreMessageCommand {
    pub request: StoreMessageRequest,
}

/// 消息编排层 StoreMessage 用例处理器，负责执行业务 Hook、WAL 及 Kafka 投递。
pub struct StoreMessageCommandHandler {
    publisher: Arc<dyn MessageEventPublisher + Send + Sync>,
    wal_repository: Arc<dyn WalRepository + Send + Sync>,
    defaults: MessageDefaults,
    hooks: Arc<HookDispatcher>,
}

impl StoreMessageCommandHandler {
    pub fn new(
        publisher: Arc<dyn MessageEventPublisher + Send + Sync>,
        wal_repository: Arc<dyn WalRepository + Send + Sync>,
        defaults: MessageDefaults,
        hooks: Arc<HookDispatcher>,
    ) -> Self {
        Self {
            publisher,
            wal_repository,
            defaults,
            hooks,
        }
    }

    /// 按照“PreSend Hook → WAL → Kafka → PostSend Hook”的顺序编排消息写入流程。
    pub async fn handle(&self, command: StoreMessageCommand) -> Result<String> {
        let mut request = command.request;

        let original_context =
            build_hook_context(&request, self.defaults.default_tenant_id.as_ref());
        let mut draft = build_draft_from_request(&request);
        self.hooks.pre_send(&original_context, &mut draft).await?;
        apply_draft_to_request(&mut request, &draft);

        let updated_context =
            build_hook_context(&request, self.defaults.default_tenant_id.as_ref());
        let hook_context = merge_context(&original_context, updated_context);

        let submission = MessageSubmission::prepare(request, &self.defaults).map_err(|err| {
            ErrorBuilder::new(ErrorCode::InvalidParameter, "failed to prepare message")
                .details(err.to_string())
                .build_error()
        })?;

        self.wal_repository
            .append(&submission)
            .await
            .map_err(|err| {
                ErrorBuilder::new(ErrorCode::InternalError, "failed to append wal entry")
                    .details(err.to_string())
                    .build_error()
            })?;

        self.publisher
            .publish(submission.kafka_payload.clone())
            .await
            .map_err(|err| {
                ErrorBuilder::new(
                    ErrorCode::ServiceUnavailable,
                    "failed to publish message event",
                )
                .details(err.to_string())
                .build_error()
            })?;

        let record = build_message_record(&submission, &submission.kafka_payload);
        let post_draft = draft_from_submission(&submission);

        self.hooks
            .post_send(&hook_context, &record, &post_draft)
            .await?;

        Ok(submission.message_id)
    }
}
